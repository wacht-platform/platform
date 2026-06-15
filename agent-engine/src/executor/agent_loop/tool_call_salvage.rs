//! Best-effort recovery of tool calls the provider's tool parser failed to
//! lift out of the assistant text. vLLM's Qwen parsers (and others) choke when a
//! parameter value carries the parser's own delimiters — shell metachars like
//! `>`, `&&`, `|`, quotes — and leave the whole call as raw markup in `content`.
//! Rather than discard the turn, we re-parse that markup leniently here and hand
//! back proper calls. Anything we can't parse cleanly yields nothing, so the
//! caller falls back to reject-and-retry; anything we mis-parse still has to pass
//! schema validation downstream, so a bad salvage fails safe as an invalid call.

use crate::llm::GeneratedToolCall;
use serde_json::{Map, Value};

/// Outcome of healing leaked markup: the calls we recovered, plus any genuine
/// prose that sat beside the markup (markup span excised). Mirrors the two
/// findings from production practice — recover the call, preserve the prose.
pub(crate) struct Salvage {
    pub calls: Vec<GeneratedToolCall>,
    pub residual_text: Option<String>,
}

/// Heal a leaked-markup assistant message: extract the calls and return the
/// surrounding prose with the markup positionally excised.
pub(crate) fn salvage(text: &str) -> Salvage {
    Salvage {
        calls: salvage_tool_calls(text),
        residual_text: strip_markup_span(text),
    }
}

/// Remove the markup span (first start-tag → last end-tag) and return whatever
/// prose remains. None when nothing meaningful is left, or when the leak is a
/// JSON-marker format with no tag boundary to anchor on (drop the residual
/// rather than risk keeping a fragment). Positional only — never re-serializes,
/// so special chars in values are untouched.
fn strip_markup_span(text: &str) -> Option<String> {
    const STARTS: [&str; 3] = ["<tool_call>", "<function=", "<arg_key>"];
    const ENDS: [&str; 4] = [
        "</tool_call>",
        "</function>",
        "</parameter>",
        "</arg_value>",
    ];
    let start = STARTS.iter().filter_map(|m| text.find(m)).min()?;
    let end = ENDS
        .iter()
        .filter_map(|m| text.rfind(m).map(|i| i + m.len()))
        .max()
        .unwrap_or(text.len())
        .max(start);

    let mut residual = text[..start].trim_end().to_string();
    let tail = text[end..].trim_start();
    if !tail.is_empty() {
        if !residual.is_empty() {
            residual.push('\n');
        }
        residual.push_str(tail);
    }
    let residual = residual.trim();
    (!residual.is_empty()).then(|| residual.to_string())
}

/// Recover whatever tool calls can be confidently reconstructed from leaked
/// markup. Empty when nothing parses.
fn salvage_tool_calls(text: &str) -> Vec<GeneratedToolCall> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    // Qwen XML: <function=NAME><parameter=KEY>VALUE</parameter>. Tolerant of
    // missing close tags and of the parser-breaking metachars in VALUE.
    let xml = salvage_qwen_xml(text);
    if !xml.is_empty() {
        return xml;
    }

    // GLM: <tool_call>NAME <arg_key>k</arg_key><arg_value>v</arg_value>.
    let glm = salvage_glm(text);
    if !glm.is_empty() {
        return glm;
    }

    // DeepSeek: name sits outside the JSON, after <｜tool▁sep｜>.
    let deepseek = salvage_deepseek(text);
    if !deepseek.is_empty() {
        return deepseek;
    }

    // Everything else is "marker(s) + JSON" (Hermes, Llama, Mistral, Cohere,
    // Granite, OpenAI-style, ReAct fences). Markers are invalid JSON, so we just
    // scan for balanced JSON values and map the ones shaped like a call.
    let mut calls = Vec::new();
    for value in extract_json_values(text) {
        collect_json_calls(&value, &mut calls);
    }
    calls
}

fn salvage_qwen_xml(text: &str) -> Vec<GeneratedToolCall> {
    let mut calls = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = text[cursor..].find("<function=") {
        let name_start = cursor + rel + "<function=".len();
        let name_rest = &text[name_start..];
        let name_len = name_rest
            .find(|c: char| c == '>' || c == '<' || c.is_whitespace())
            .unwrap_or(name_rest.len());
        let name = name_rest[..name_len].trim().to_string();
        let region_start = name_start + name_len;
        // This call owns everything up to the next function marker.
        let region_end = text[region_start..]
            .find("<function=")
            .map(|i| region_start + i)
            .unwrap_or(text.len());
        if !name.is_empty() {
            let arguments = parse_xml_parameters(&text[region_start..region_end]);
            calls.push(GeneratedToolCall {
                tool_name: name,
                arguments: Value::Object(arguments),
                signature: None,
            });
        }
        cursor = region_end;
    }
    calls
}

/// XML/GLM parameter values arrive as raw strings, but some tools expect
/// structured arguments (e.g. an array or object parameter such as
/// `ask_user.questions`). Coerce values that look like JSON to the real thing so
/// downstream schema validation passes; anything else stays a string. Mirrors the
/// snippet harness fix for the same leak shape.
fn coerce_value(raw: &str) -> Value {
    let trimmed = raw.trim();
    let looks_json = (trimmed.starts_with('[') && trimmed.ends_with(']'))
        || (trimmed.starts_with('{') && trimmed.ends_with('}'));
    if looks_json {
        if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
            return value;
        }
    }
    Value::String(trimmed.to_string())
}

fn parse_xml_parameters(region: &str) -> Map<String, Value> {
    const DELIMS: [&str; 4] = ["</parameter>", "<parameter=", "</function>", "</tool_call>"];
    let mut args = Map::new();
    let mut cursor = 0;
    while let Some(rel) = region[cursor..].find("<parameter=") {
        let key_start = cursor + rel + "<parameter=".len();
        let key_rest = &region[key_start..];
        let key_len = key_rest
            .find(|c: char| c == '>' || c.is_whitespace())
            .unwrap_or(key_rest.len());
        let key = key_rest[..key_len].trim().to_string();
        // Step past the opening tag's '>' to where the value starts.
        let mut value_start = key_start + key_len;
        match region[value_start..].find('>') {
            Some(gt) => value_start += gt + 1,
            None => {} // malformed open tag — take from right after the key
        }
        let rest = &region[value_start..];
        let value_end = DELIMS
            .iter()
            .filter_map(|d| rest.find(d))
            .min()
            .unwrap_or(rest.len());
        if !key.is_empty() {
            args.insert(key, coerce_value(&rest[..value_end]));
        }
        cursor = value_start + value_end;
    }
    args
}

fn salvage_glm(text: &str) -> Vec<GeneratedToolCall> {
    if !text.contains("<arg_key>") {
        return Vec::new();
    }
    let after = match text.find("<tool_call>") {
        Some(i) => &text[i + "<tool_call>".len()..],
        None => return Vec::new(),
    };
    let name_len = after
        .find(|c: char| c == '<' || c == '\n')
        .unwrap_or(after.len());
    let name = after[..name_len].trim().to_string();
    if name.is_empty() {
        return Vec::new();
    }

    let mut args = Map::new();
    let mut cursor = 0;
    while let Some(rel) = text[cursor..].find("<arg_key>") {
        let key_start = cursor + rel + "<arg_key>".len();
        let Some(key_end) = text[key_start..].find("</arg_key>").map(|i| key_start + i) else {
            break;
        };
        let key = text[key_start..key_end].trim().to_string();
        let Some(val_start) = text[key_end..]
            .find("<arg_value>")
            .map(|i| key_end + i + "<arg_value>".len())
        else {
            break;
        };
        let val_end = text[val_start..]
            .find("</arg_value>")
            .map(|i| val_start + i)
            .unwrap_or(text.len());
        if !key.is_empty() {
            args.insert(key, coerce_value(&text[val_start..val_end]));
        }
        cursor = val_end;
    }
    if args.is_empty() {
        return Vec::new();
    }
    vec![GeneratedToolCall {
        tool_name: name,
        arguments: Value::Object(args),
        signature: None,
    }]
}

fn salvage_deepseek(text: &str) -> Vec<GeneratedToolCall> {
    const SEP: &str = "<｜tool▁sep｜>";
    let mut calls = Vec::new();
    let mut cursor = 0;
    while let Some(rel) = text[cursor..].find(SEP) {
        let name_start = cursor + rel + SEP.len();
        let name_rest = &text[name_start..];
        let name_len = name_rest
            .find(|c: char| c.is_whitespace() || c == '<')
            .unwrap_or(name_rest.len());
        let name = name_rest[..name_len].trim().to_string();
        let arguments = extract_json_values(&text[name_start + name_len..])
            .into_iter()
            .find(Value::is_object)
            .unwrap_or_else(|| Value::Object(Map::new()));
        if !name.is_empty() {
            calls.push(GeneratedToolCall {
                tool_name: name,
                arguments,
                signature: None,
            });
        }
        cursor = name_start + name_len;
    }
    calls
}

/// Flatten a parsed JSON value into calls. Accepts arrays, OpenAI-style
/// `{function: {...}}`, and the common name/arguments key aliases.
fn collect_json_calls(value: &Value, out: &mut Vec<GeneratedToolCall>) {
    match value {
        Value::Array(items) => items.iter().for_each(|i| collect_json_calls(i, out)),
        Value::Object(obj) => {
            if let Some(inner) = obj.get("function").filter(|f| f.is_object()) {
                return collect_json_calls(inner, out);
            }
            let name = ["name", "tool_name", "tool"]
                .iter()
                .find_map(|k| obj.get(*k).and_then(Value::as_str))
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let Some(name) = name else { return };
            let arguments = ["arguments", "parameters", "args"]
                .iter()
                .find_map(|k| obj.get(*k).cloned())
                .map(|v| match v {
                    // arguments are sometimes a JSON-encoded string.
                    Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
                    other => other,
                })
                .unwrap_or_else(|| Value::Object(Map::new()));
            out.push(GeneratedToolCall {
                tool_name: name.to_string(),
                arguments,
                signature: None,
            });
        }
        _ => {}
    }
}

/// Scan for balanced, parseable JSON objects/arrays. Non-JSON framing markers
/// fail to parse and are skipped.
fn extract_json_values(text: &str) -> Vec<Value> {
    let bytes = text.as_bytes();
    let mut values = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' || bytes[i] == b'[' {
            if let Some((value, end)) = parse_balanced(text, i) {
                values.push(value);
                i = end;
                continue;
            }
        }
        i += 1;
    }
    values
}

fn parse_balanced(text: &str, start: usize) -> Option<(Value, usize)> {
    let bytes = text.as_bytes();
    let (open, close) = if bytes[start] == b'{' {
        (b'{', b'}')
    } else {
        (b'[', b']')
    };
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let c = bytes[i];
        if in_str {
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return serde_json::from_str::<Value>(&text[start..=i])
                    .ok()
                    .map(|v| (v, i + 1));
            }
        }
        i += 1;
    }
    None
}
