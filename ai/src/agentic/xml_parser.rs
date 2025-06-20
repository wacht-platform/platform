use super::ToolCall;
use serde_json::{Value, json};
use quick_xml::events::{Event, BytesStart, BytesEnd, BytesText};
use quick_xml::Reader;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct XmlParser {
    buffer: String,
}

impl XmlParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    pub fn parse_chunk(&mut self, chunk: &str) -> (Option<String>, Vec<ToolCall>) {
        self.buffer.push_str(chunk);

        let mut text_content = String::new();
        let mut new_tools = Vec::new();

        // Look for complete tool calls
        while let Some(start) = self.buffer.find("<tool_call>") {
            // Add any text before the tool call to content
            if start > 0 {
                text_content.push_str(&self.buffer[..start]);
            }

            // Find the end of the tool call
            if let Some(end) = self.buffer.find("</tool_call>") {
                let tool_xml = &self.buffer[start..end + 12]; // Include closing tag

                if let Some(tool_call) = self.parse_tool_call_xml(tool_xml) {
                    new_tools.push(tool_call);
                }

                // Remove the processed tool call from buffer
                self.buffer = self.buffer[end + 12..].to_string();
            } else {
                // Incomplete tool call, keep in buffer
                self.buffer = self.buffer[start..].to_string();
                break;
            }
        }

        // If no more tool calls, add remaining buffer to text content
        if !self.buffer.contains("<tool_call>") && !self.buffer.is_empty() {
            text_content.push_str(&self.buffer);
            self.buffer.clear();
        }

        let content = if text_content.is_empty() { None } else { Some(text_content) };
        (content, new_tools)
    }

    fn parse_tool_call_xml(&self, xml: &str) -> Option<ToolCall> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut name = String::new();
        let mut id = String::new();
        let mut arguments = HashMap::new();

        let mut current_element = String::new();
        let mut current_arg_name = String::new();
        let mut in_arguments = false;
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let element_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    current_element = element_name.clone();

                    if element_name == "arguments" {
                        in_arguments = true;
                    } else if in_arguments && element_name != "tool_call" {
                        current_arg_name = element_name;
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();

                    match current_element.as_str() {
                        "name" => name = text,
                        "id" => id = text,
                        _ if in_arguments && !current_arg_name.is_empty() => {
                            arguments.insert(current_arg_name.clone(), text);
                        }
                        _ => {}
                    }
                }
                Ok(Event::End(ref e)) => {
                    let element_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if element_name == "arguments" {
                        in_arguments = false;
                    } else if element_name == "tool_call" {
                        break;
                    }
                    current_element.clear();
                    current_arg_name.clear();
                }
                Ok(Event::Eof) => break,
                Err(_) => return None,
                _ => {}
            }
            buf.clear();
        }

        if name.is_empty() {
            return None;
        }

        if id.is_empty() {
            id = format!("tool_{}", uuid::Uuid::new_v4());
        }

        // Convert HashMap to JSON Value
        let args_json = if arguments.is_empty() {
            json!({})
        } else {
            let mut json_map = serde_json::Map::new();
            for (key, value) in arguments {
                json_map.insert(key, Value::String(value));
            }
            Value::Object(json_map)
        };

        Some(ToolCall {
            id,
            name,
            arguments: args_json,
        })
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
    }
}
