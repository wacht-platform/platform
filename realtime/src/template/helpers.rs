use handlebars::{
    Context, Handlebars, Helper, HelperResult, Output, RenderContext, RenderErrorReason,
};
use serde_json::Value;

pub fn register_all_helpers(hb: &mut Handlebars) {
    hb.register_helper("format_tools", Box::new(FormatToolsHelper));
    hb.register_helper("format_workflows", Box::new(FormatWorkflowsHelper));
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
    hb.register_helper("format_capabilities", Box::new(FormatCapabilitiesHelper));
    hb.register_helper(
        "format_dynamic_context",
        Box::new(FormatDynamicContextHelper),
    );
    hb.register_helper("current_timestamp", Box::new(CurrentTimestampHelper));
    hb.register_helper("eq", Box::new(EqHelper));
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
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let description = tool
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No description");
                format!("- {}: {}", name, description)
            })
            .collect();

        out.write(&formatted_tools.join("\n"))?;
        Ok(())
    }
}

pub struct FormatWorkflowsHelper;

impl handlebars::HelperDef for FormatWorkflowsHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let workflows = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected workflows array"))?;

        let formatted_workflows: Vec<String> = workflows
            .iter()
            .map(|workflow| {
                let name = workflow
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let description = workflow
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No description");
                format!("- {}: {}", name, description)
            })
            .collect();

        out.write(&formatted_workflows.join("\n"))?;
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
                format!("- {}: {}", name, description)
            })
            .collect();

        out.write(&formatted_kbs.join("\n"))?;
        Ok(())
    }
}

pub struct FormatDynamicContextHelper;

impl handlebars::HelperDef for FormatDynamicContextHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let context_items = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected dynamic context array"))?;

        let formatted_items: Vec<String> = context_items
            .iter()
            .map(|item| {
                let content = item
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No content");
                let source = item
                    .get("source")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown source");
                format!("- [{}] {}", source, content)
            })
            .collect();

        out.write(&formatted_items.join("\n"))?;
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
                let importance = memory
                    .get("importance")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                format!("- {} (importance: {:.2})", content, importance)
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
                format!("{}: {}", key, value_str)
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

pub struct FormatCapabilitiesHelper;

impl handlebars::HelperDef for FormatCapabilitiesHelper {
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let empty_vec = vec![];
        let tools = h
            .param(0)
            .and_then(|v| v.value().as_array())
            .unwrap_or(&empty_vec);
        let workflows = h
            .param(1)
            .and_then(|v| v.value().as_array())
            .unwrap_or(&empty_vec);
        let knowledge_bases = h
            .param(2)
            .and_then(|v| v.value().as_array())
            .unwrap_or(&empty_vec);

        let mut output = String::new();

        if !tools.is_empty() {
            output.push_str("Tools Available:\n");
            for tool in tools {
                let name = tool
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let description = tool
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No description");
                output.push_str(&format!("- {}: {}\n", name, description));
            }
            output.push('\n');
        }

        if !workflows.is_empty() {
            output.push_str("Workflows Available:\n");
            for workflow in workflows {
                let name = workflow
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown");
                let description = workflow
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No description");
                output.push_str(&format!("- {}: {}\n", name, description));
            }
            output.push('\n');
        }

        if !knowledge_bases.is_empty() {
            output.push_str("Knowledge Bases Available:\n");
            for kb in knowledge_bases {
                let name = kb.get("name").and_then(|v| v.as_str()).unwrap_or("Unknown");
                let description = kb
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("No description");
                output.push_str(&format!("- {}: {}\n", name, description));
            }
        }

        out.write(&output.trim_end())?;
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
    fn call<'reg: 'rc, 'rc>(
        &self,
        h: &Helper,
        _: &Handlebars,
        _: &Context,
        _: &mut RenderContext,
        out: &mut dyn Output,
    ) -> HelperResult {
        let param1 = h
            .param(0)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected first parameter"))?
            .value();
        let param2 = h
            .param(1)
            .ok_or_else(|| RenderErrorReason::InvalidParamType("Expected second parameter"))?
            .value();

        // For now, just return true/false as string for simplicity
        // The template should handle the condition differently
        let result = match (param1, param2) {
            (Value::String(s1), Value::String(s2)) => s1 == s2,
            (Value::Number(n1), Value::Number(n2)) => n1 == n2,
            (Value::Bool(b1), Value::Bool(b2)) => b1 == b2,
            (Value::Null, Value::Null) => true,
            _ => false,
        };

        out.write(if result { "true" } else { "false" })?;
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
                        parts.push(format!("{}{}:", indent, key));
                        parts.push(json_to_flat_string(val, indent_level + 1));
                    }
                    _ => {
                        // For primitive values, show key: value
                        let val_str = match val {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            Value::Null => "null".to_string(),
                            _ => format!("{:?}", val),
                        };
                        parts.push(format!("{}{}: {}", indent, key, val_str));
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
                            _ => format!("{:?}", val),
                        };
                        parts.push(format!("{}{}", indent, val_str));
                    }
                }
            }
            parts.join("\n")
        }
        _ => {
            // For primitive values at root
            match value {
                Value::String(s) => format!("{}{}", indent, s),
                Value::Number(n) => format!("{}{}", indent, n),
                Value::Bool(b) => format!("{}{}", indent, b),
                Value::Null => format!("{}null", indent),
                _ => format!("{}{:?}", indent, value),
            }
        }
    }
}
