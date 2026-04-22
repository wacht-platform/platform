use chrono::{DateTime, Utc};
use handlebars::{
    Context, Handlebars, Helper, HelperDef, HelperResult, Output, RenderContext, RenderError,
    RenderErrorReason, ScopedJson,
};
use serde_json::Value;

pub fn register_all_helpers(hb: &mut Handlebars) {
    hb.register_helper("format_tools", Box::new(FormatToolsHelper));
    hb.register_helper(
        "format_knowledge_bases",
        Box::new(FormatKnowledgeBasesHelper),
    );
    hb.register_helper("format_memories", Box::new(FormatMemoriesHelper));
    hb.register_helper("format_map", Box::new(FormatMapHelper));
    hb.register_helper("join", Box::new(JoinHelper));
    hb.register_helper("json", Box::new(JsonHelper));
    hb.register_helper("json_pretty", Box::new(JsonPrettyHelper));
    hb.register_helper("json_string", Box::new(JsonStringHelper));
    hb.register_helper("truncate", Box::new(TruncateHelper));
    hb.register_helper("default", Box::new(DefaultHelper));
    hb.register_helper("current_timestamp", Box::new(CurrentTimestampHelper));
    hb.register_helper("eq", Box::new(EqHelper));
    hb.register_helper("has_any_tool", Box::new(HasAnyToolHelper));
    hb.register_helper("format_timestamp", Box::new(FormatTimestampHelper));
    hb.register_helper("relative_time", Box::new(RelativeTimeHelper));
}

pub struct FormatToolsHelper;

impl handlebars::HelperDef for FormatToolsHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let tools = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected tools array"))?;

        let formatted_tools: Vec<String> = tools
            .iter()
            .map(|tool| {
                let name = tool
                    .get("name")
                    .or_else(|| tool.get("slug"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let description = tool
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("---");
                let mount_path = tool.get("mount_path").and_then(|v| v.as_str());
                let input_fields = tool
                    .get("input_schema")
                    .and_then(|v| v.as_array())
                    .map(|fields| {
                        fields
                            .iter()
                            .filter_map(|field| {
                                let field_name = field.get("name").and_then(|v| v.as_str())?;
                                let field_type = field
                                    .get("field_type")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("ANY")
                                    .to_lowercase();
                                let required = field
                                    .get("required")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let description = field
                                    .get("description")
                                    .and_then(|v| v.as_str())
                                    .map(|desc| {
                                        let mut shortened = desc.trim().to_string();
                                        if let Some((first, _)) = shortened.split_once('.') {
                                            shortened = first.trim().to_string();
                                        }
                                        if shortened.chars().count() > 72 {
                                            shortened =
                                                shortened.chars().take(72).collect::<String>();
                                            shortened.push_str("...");
                                        }
                                        shortened
                                    });
                                let mut rendered = if required {
                                    format!("{field_name}*<{field_type}>")
                                } else {
                                    format!("{field_name}<{field_type}>")
                                };
                                if let Some(desc) = description {
                                    if !desc.is_empty() {
                                        rendered.push_str(" - ");
                                        rendered.push_str(&desc);
                                    }
                                }
                                Some(rendered)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                if input_fields.is_empty() {
                    if let Some(mount_path) = mount_path {
                        format!("- {name} at `{mount_path}`: {description}")
                    } else {
                        format!("- {name}: {description}")
                    }
                } else {
                    format!(
                        "- {name}: {description} Inputs: {}",
                        input_fields.join(", ")
                    )
                }
            })
            .collect();

        out.write(&formatted_tools.join("\n"))?;
        Ok(())
    }
}

pub struct FormatKnowledgeBasesHelper;

impl handlebars::HelperDef for FormatKnowledgeBasesHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let knowledge_bases = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected knowledge_bases array"))?;

        let formatted_kbs: Vec<String> = knowledge_bases
            .iter()
            .map(|kb| {
                let name = kb.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown");
                let description = kb
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No description");
                format!("- {name}: {description}")
            })
            .collect();

        out.write(&formatted_kbs.join("\n"))?;
        Ok(())
    }
}

pub struct FormatMemoriesHelper;

impl handlebars::HelperDef for FormatMemoriesHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let memories = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected memories array"))?;

        let formatted_memories: Vec<String> = memories
            .iter()
            .map(|memory| {
                let content = memory
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No content");
                let category = memory
                    .get("memory_category")
                    .or_else(|| memory.get("category"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let created_at = memory
                    .get("created_at")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                let scope = memory
                    .get("memory_scope")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!("- [{category}][{scope}] {content} (created_at: {created_at})")
            })
            .collect();

        out.write(&formatted_memories.join("\n"))?;
        Ok(())
    }
}

pub struct FormatMapHelper;

impl handlebars::HelperDef for FormatMapHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let map = h
            .param(0)
            .and_then(|v| v.value().as_object())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected object/map"))?;

        let separator = h.param(1).and_then(|v| v.value().as_str()).unwrap_or("\n");

        let formatted_pairs: Vec<String> = map
            .iter()
            .map(|(key, value)| {
                let value_str = match value {
                    Value::String(s) => s.clone(),
                    _ => serde_json::to_string(value).unwrap_or_default(),
                };
                format!("{key}: {value_str}")
            })
            .collect();

        out.write(&formatted_pairs.join(separator))?;
        Ok(())
    }
}

pub struct JoinHelper;

impl handlebars::HelperDef for JoinHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let array = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected array"))?;

        let separator = h.param(1).and_then(|v| v.value().as_str()).unwrap_or(", ");

        let strings: Vec<String> = array
            .iter()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                _ => serde_json::to_string(v).unwrap_or_default(),
            })
            .collect();

        out.write(&strings.join(separator))?;
        Ok(())
    }
}

pub struct JsonHelper;

impl handlebars::HelperDef for JsonHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let value = h
            .param(0)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected value"))?
            .value();

        let json_string = serde_json::to_string(value)
            .map_err(|_| RenderErrorReason::InvalidParamType("Failed to serialize to JSON"))?;

        out.write(&json_string)?;
        Ok(())
    }
}

pub struct JsonPrettyHelper;

impl handlebars::HelperDef for JsonPrettyHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let value = h
            .param(0)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected value"))?
            .value();

        let pretty_json = serde_json::to_string_pretty(value)
            .map_err(|_| RenderErrorReason::InvalidParamType("Failed to serialize to JSON"))?;

        out.write(&pretty_json)?;
        Ok(())
    }
}

pub struct TruncateHelper;

impl handlebars::HelperDef for TruncateHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let text = h
            .param(0)
            .and_then(|v| v.value().as_str())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected string"))?;

        let max_length = h.param(1).and_then(|v| v.value().as_u64()).unwrap_or(100) as usize;

        let truncated = if text.len() > max_length {
            format!("{}...", &text[..max_length.saturating_sub(3)])
        } else {
            text.to_string()
        };

        out.write(&truncated)?;
        Ok(())
    }
}

pub struct DefaultHelper;

impl handlebars::HelperDef for DefaultHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let value = h.param(0).map(|v| v.value());
        let default_value = h.param(1).and_then(|v| v.value().as_str()).unwrap_or("");

        let output = match value {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            Some(Value::Null) | None => default_value.to_string(),
            Some(v) => serde_json::to_string(v).unwrap_or_else(|_| default_value.to_string()),
        };

        out.write(&output)?;
        Ok(())
    }
}

pub struct CurrentTimestampHelper;

impl handlebars::HelperDef for CurrentTimestampHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        _h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let timestamp = chrono::Utc::now().to_rfc3339();
        out.write(&timestamp)?;
        Ok(())
    }
}

pub struct EqHelper;

impl handlebars::HelperDef for EqHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'rc>, RenderError> {
        let param1 = h
            .param(0)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected first parameter"))?
            .value();
        let param2 = h
            .param(1)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected second parameter"))?
            .value();

        let result = match (param1, param2) {
            (Value::String(s1), Value::String(s2)) => s1 == s2,
            (Value::Number(n1), Value::Number(n2)) => n1 == n2,
            (Value::Bool(b1), Value::Bool(b2)) => b1 == b2,
            (Value::Null, Value::Null) => true,
            _ => false,
        };

        Ok(ScopedJson::Derived(Value::Bool(result)))
    }

    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        r: &'reg Handlebars<'reg>,
        ctx: &'rc Context,
        rc: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        let value = self.call_inner(h, r, ctx, rc)?;
        out.write(&value.render())?;
        Ok(())
    }
}

pub struct HasAnyToolHelper;

impl HelperDef for HasAnyToolHelper {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        _: &'reg Handlebars<'reg>,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'rc>, RenderError> {
        let tools = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .cloned()
            .unwrap_or_default();

        let names: Vec<&str> = h
            .params()
            .iter()
            .skip(1)
            .filter_map(|param| param.value().as_str())
            .collect();

        if names.is_empty() {
            return Err(
                RenderErrorReason::InvalidParamType("Expected at least one tool name").into(),
            );
        }

        let has_match = tools.iter().any(|tool| {
            tool.get("name")
                .and_then(|value| value.as_str())
                .map(|name| names.contains(&name))
                .unwrap_or(false)
        });

        Ok(ScopedJson::Derived(Value::Bool(has_match)))
    }

    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'rc>,
        r: &'reg Handlebars<'reg>,
        ctx: &'rc Context,
        rc: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> HelperResult {
        let value = self.call_inner(h, r, ctx, rc)?;
        out.write(&value.render())?;
        Ok(())
    }
}

pub struct JsonStringHelper;

impl handlebars::HelperDef for JsonStringHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let value = h
            .param(0)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected value"))?
            .value();

        // If the value is a string, try to parse it as JSON first
        let value_to_flatten = match value {
            Value::String(s) => {
                // Try to parse the string as JSON
                match serde_json::from_str::<Value>(s) {
                    Ok(parsed) => parsed,
                    Err(_) => value.clone(), // If parsing fails, use the original value
                }
            }
            _ => value.clone(),
        };

        // Convert the value to a flattened key-value string representation
        let result = json_to_flat_string(&value_to_flatten, 0);

        // Escape the result for JSON string usage
        let escaped = result
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");

        // Write the escaped string
        out.write(&escaped)?;
        Ok(())
    }
}

fn json_to_flat_string(value: &Value, indent_level: usize) -> String {
    let indent = "  ".repeat(indent_level);

    match value {
        Value::Object(map) => {
            let mut parts = Vec::new();
            for (key, val) in map {
                match val {
                    Value::Object(_) | Value::Array(_) => {
                        // For nested objects or arrays, show key with colon
                        parts.push(format!("{indent}{key}:"));
                        parts.push(json_to_flat_string(val, indent_level + 1));
                    }
                    _ => {
                        // For primitive values, show key: value
                        let val_str = match val {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            Value::Null => "null".to_string(),
                            _ => format!("{val:?}"),
                        };
                        parts.push(format!("{indent}{key}: {val_str}"));
                    }
                }
            }
            parts.join("\n")
        }
        Value::Array(arr) => {
            let mut parts = Vec::new();
            for val in arr {
                match val {
                    Value::Object(_) => {
                        // For objects in arrays, flatten them at current indent level
                        parts.push(json_to_flat_string(val, indent_level));
                    }
                    Value::Array(_) => {
                        // For nested arrays, increase indent
                        parts.push(json_to_flat_string(val, indent_level + 1));
                    }
                    _ => {
                        // For primitive values in arrays, just show the value
                        let val_str = match val {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            Value::Null => "null".to_string(),
                            _ => format!("{val:?}"),
                        };
                        parts.push(format!("{indent}{val_str}"));
                    }
                }
            }
            parts.join("\n")
        }
        _ => {
            // For primitive values at root
            match value {
                Value::String(s) => format!("{indent}{s}"),
                Value::Number(n) => format!("{indent}{n}"),
                Value::Bool(b) => format!("{indent}{b}"),
                Value::Null => format!("{indent}null"),
                _ => format!("{indent}{value:?}"),
            }
        }
    }
}

pub struct FormatTimestampHelper;

impl handlebars::HelperDef for FormatTimestampHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let timestamp = h
            .param(0)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected timestamp"))?
            .value();

        // Handle different timestamp formats
        let datetime = match timestamp {
            Value::String(s) => {
                // Try to parse ISO 8601 format
                DateTime::parse_from_rfc3339(s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .or_else(|_| {
                        DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                            .map(|dt| dt.with_timezone(&Utc))
                    })
                    .map_err(|_| RenderErrorReason::InvalidParamType("Invalid timestamp format"))?
            }
            Value::Number(n) => {
                // Assume it's a Unix timestamp
                let secs = n.as_i64().ok_or_else(|| {
                    RenderErrorReason::InvalidParamType("Invalid timestamp number")
                })?;
                DateTime::from_timestamp(secs, 0)
                    .ok_or_else(|| RenderErrorReason::InvalidParamType("Invalid timestamp value"))?
            }
            _ => {
                return Err(RenderErrorReason::InvalidParamType(
                    "Timestamp must be string or number",
                )
                .into());
            }
        };

        // Format as "YYYY-MM-DD HH:MM:SS UTC"
        let formatted = datetime.format("%Y-%m-%d %H:%M:%S UTC").to_string();
        out.write(&formatted)?;
        Ok(())
    }
}

pub struct RelativeTimeHelper;

impl handlebars::HelperDef for RelativeTimeHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let timestamp = h
            .param(0)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected timestamp"))?
            .value();

        // Parse timestamp (same as above)
        let datetime = match timestamp {
            Value::String(s) => DateTime::parse_from_rfc3339(s)
                .map(|dt| dt.with_timezone(&Utc))
                .or_else(|_| {
                    DateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S")
                        .map(|dt| dt.with_timezone(&Utc))
                })
                .map_err(|_| RenderErrorReason::InvalidParamType("Invalid timestamp format"))?,
            Value::Number(n) => {
                let secs = n.as_i64().ok_or_else(|| {
                    RenderErrorReason::InvalidParamType("Invalid timestamp number")
                })?;
                DateTime::from_timestamp(secs, 0)
                    .ok_or_else(|| RenderErrorReason::InvalidParamType("Invalid timestamp value"))?
            }
            _ => {
                return Err(RenderErrorReason::InvalidParamType(
                    "Timestamp must be string or number",
                )
                .into());
            }
        };

        let now = Utc::now();
        let diff = now.signed_duration_since(datetime);

        let relative = if diff.num_seconds() < 60 {
            "just now".to_string()
        } else if diff.num_minutes() < 60 {
            format!(
                "{} minute{} ago",
                diff.num_minutes(),
                if diff.num_minutes() == 1 { "" } else { "s" }
            )
        } else if diff.num_hours() < 24 {
            format!(
                "{} hour{} ago",
                diff.num_hours(),
                if diff.num_hours() == 1 { "" } else { "s" }
            )
        } else if diff.num_days() < 7 {
            format!(
                "{} day{} ago",
                diff.num_days(),
                if diff.num_days() == 1 { "" } else { "s" }
            )
        } else if diff.num_weeks() < 4 {
            format!(
                "{} week{} ago",
                diff.num_weeks(),
                if diff.num_weeks() == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "{} month{} ago",
                diff.num_days() / 30,
                if diff.num_days() / 30 == 1 { "" } else { "s" }
            )
        };

        out.write(&relative)?;
        Ok(())
    }
}
