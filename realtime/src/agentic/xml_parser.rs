use super::ToolCall;
use serde_json::{Value, json};
use std::collections::HashMap;
use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Clone)]
pub struct XmlElement {
    pub tag: String,
    pub attributes: HashMap<String, String>,
    pub text_content: String,
    pub children: Vec<XmlElement>,
}

#[derive(Debug, Clone)]
struct ElementState {
    tag: String,
    attributes: HashMap<String, String>,
    content: String,
    start_pos: usize,
}

#[derive(Debug, Clone)]
pub struct XmlParser {
    buffer: String,
    max_buffer_size: usize,
    element_stack: Vec<ElementState>,    // Stack for tracking open elements
    completed_elements: Vec<XmlElement>, // Completed root elements
}

impl XmlElement {
    pub fn new(tag: String) -> Self {
        Self {
            tag,
            attributes: HashMap::new(),
            text_content: String::new(),
            children: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Value {
        let mut obj = serde_json::Map::new();

        // Add tag name
        obj.insert("_tag".to_string(), Value::String(self.tag.clone()));

        // Add attributes
        if !self.attributes.is_empty() {
            let attrs: serde_json::Map<String, Value> = self
                .attributes
                .iter()
                .map(|(k, v)| (format!("@{}", k), Value::String(v.clone())))
                .collect();
            obj.extend(attrs);
        }

        // Add text content
        if !self.text_content.trim().is_empty() {
            obj.insert(
                "_text".to_string(),
                Value::String(self.text_content.clone()),
            );
        }

        // Add children
        if !self.children.is_empty() {
            for child in &self.children {
                let child_json = child.to_json();
                obj.insert(child.tag.clone(), child_json);
            }
        }

        Value::Object(obj)
    }
}

impl XmlParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            max_buffer_size: 1024 * 1024, // 1MB default limit
            element_stack: Vec::new(),
            completed_elements: Vec::new(),
        }
    }

    pub fn with_max_buffer_size(mut self, size: usize) -> Self {
        self.max_buffer_size = size;
        self
    }

    /// Get current buffer size for monitoring
    pub fn buffer_size(&self) -> usize {
        self.buffer.len()
    }

    /// Get number of completed elements
    pub fn completed_count(&self) -> usize {
        self.completed_elements.len()
    }

    /// Get completed elements and clear them
    pub fn take_completed_elements(&mut self) -> Vec<XmlElement> {
        std::mem::take(&mut self.completed_elements)
    }

    /// Clear the buffer and reset parser state (useful for error recovery)
    pub fn clear_buffer(&mut self) {
        self.buffer.clear();
        self.element_stack.clear();
        self.completed_elements.clear();
    }

    /// Parse any XML chunk dynamically using fast string operations and stack
    pub fn parse_chunk(&mut self, chunk: &str) -> Result<Vec<XmlElement>, shared::error::AppError> {
        // Check buffer size limit to prevent memory exhaustion
        if self.buffer.len() + chunk.len() > self.max_buffer_size {
            return Err(shared::error::AppError::Internal(format!(
                "XML parser buffer size limit exceeded: {} bytes",
                self.max_buffer_size
            )));
        }

        self.buffer.push_str(chunk);
        let mut completed_elements = Vec::new();
        let mut processed_up_to = 0;

        // Process the buffer looking for complete XML elements
        while let Some(complete_element_end) = self.find_complete_element(processed_up_to) {
            if let Some(element) = self.extract_element(processed_up_to, complete_element_end) {
                completed_elements.push(element);
            }
            processed_up_to = complete_element_end;
        }

        // Remove processed content from buffer
        if processed_up_to > 0 {
            self.buffer = self.buffer[processed_up_to..].to_string();
        }

        // Store completed elements
        self.completed_elements.extend(completed_elements.clone());

        Ok(completed_elements)
    }

    /// Find the end position of a complete XML element starting from the given position
    fn find_complete_element(&self, start_pos: usize) -> Option<usize> {
        let buffer = &self.buffer[start_pos..];
        let mut tag_stack = Vec::new();
        let mut pos = 0;

        while pos < buffer.len() {
            if let Some(tag_start) = buffer[pos..].find('<') {
                let abs_tag_start = pos + tag_start;

                if let Some(tag_end) = buffer[abs_tag_start..].find('>') {
                    let abs_tag_end = abs_tag_start + tag_end + 1;
                    let tag_content = &buffer[abs_tag_start + 1..abs_tag_start + tag_end];

                    if tag_content.starts_with('/') {
                        // Closing tag
                        let tag_name = &tag_content[1..];
                        if let Some(last_tag) = tag_stack.last() {
                            if last_tag == tag_name {
                                tag_stack.pop();
                                if tag_stack.is_empty() {
                                    // Found complete element
                                    return Some(start_pos + abs_tag_end);
                                }
                            }
                        }
                    } else if tag_content.ends_with('/') {
                        // Self-closing tag
                        if tag_stack.is_empty() {
                            return Some(start_pos + abs_tag_end);
                        }
                    } else {
                        // Opening tag
                        let tag_name = tag_content.split_whitespace().next().unwrap_or("");
                        tag_stack.push(tag_name.to_string());
                    }

                    pos = abs_tag_end;
                } else {
                    // Incomplete tag
                    break;
                }
            } else {
                // No more tags
                break;
            }
        }

        None
    }

    /// Extract and parse an XML element from the buffer
    fn extract_element(&self, start_pos: usize, end_pos: usize) -> Option<XmlElement> {
        let xml_content = &self.buffer[start_pos..end_pos];

        // Find the first opening tag
        if let Some(first_tag_start) = xml_content.find('<') {
            if let Some(first_tag_end) = xml_content[first_tag_start..].find('>') {
                let tag_end_abs = first_tag_start + first_tag_end + 1;
                let tag_content = &xml_content[first_tag_start + 1..first_tag_start + first_tag_end];

                // Parse tag name and attributes
                let (tag_name, attributes) = self.parse_tag_and_attributes(tag_content);

                // Check if self-closing
                if tag_content.ends_with('/') {
                    return Some(XmlElement {
                        tag: tag_name,
                        attributes,
                        text_content: String::new(),
                        children: Vec::new(),
                    });
                }

                // Find the closing tag
                let closing_tag = format!("</{}>", tag_name);
                if let Some(closing_pos) = xml_content.rfind(&closing_tag) {
                    // Extract text content between opening and closing tags
                    let content = &xml_content[tag_end_abs..closing_pos];
                    let text_content = content.trim().to_string();

                    return Some(XmlElement {
                        tag: tag_name,
                        attributes,
                        text_content,
                        children: Vec::new(), // Simplified - not handling nested elements for now
                    });
                }
            }
        }

        None
    }

    /// Parse tag name and attributes from tag content (fast string operations)
    fn parse_tag_and_attributes(&self, tag_content: &str) -> (String, HashMap<String, String>) {
        let mut attributes = HashMap::new();
        let tag_content = tag_content.trim_end_matches('/');

        // Split by whitespace but be careful with quoted values
        let mut parts = Vec::new();
        let mut current_part = String::new();
        let mut in_quotes = false;
        let mut quote_char = '"';

        for ch in tag_content.chars() {
            match ch {
                '"' | '\'' if !in_quotes => {
                    in_quotes = true;
                    quote_char = ch;
                    current_part.push(ch);
                }
                ch if ch == quote_char && in_quotes => {
                    in_quotes = false;
                    current_part.push(ch);
                }
                ' ' | '\t' | '\n' | '\r' if !in_quotes => {
                    if !current_part.is_empty() {
                        parts.push(current_part.clone());
                        current_part.clear();
                    }
                }
                _ => {
                    current_part.push(ch);
                }
            }
        }

        if !current_part.is_empty() {
            parts.push(current_part);
        }

        if parts.is_empty() {
            return (String::new(), attributes);
        }

        let tag_name = parts[0].clone();

        // Parse attributes (key="value" format)
        for part in &parts[1..] {
            if let Some(eq_pos) = part.find('=') {
                let key = part[..eq_pos].to_string();
                let value_part = &part[eq_pos + 1..];

                // Remove quotes
                let value = if (value_part.starts_with('"') && value_part.ends_with('"')) ||
                              (value_part.starts_with('\'') && value_part.ends_with('\'')) {
                    value_part[1..value_part.len() - 1].to_string()
                } else {
                    value_part.to_string()
                };

                attributes.insert(key, value);
            }
        }

        (tag_name, attributes)
    }

    /// Compatibility method for existing tool call parsing
    pub fn parse_chunk_for_tools(
        &mut self,
        chunk: &str,
    ) -> Result<(Option<String>, Vec<ToolCall>), shared::error::AppError> {
        let completed_elements = self.parse_chunk(chunk)?;

        let mut text_content = String::new();
        let mut tool_calls = Vec::new();

        for element in completed_elements {
            if element.tag == "tool_call" {
                if let Some(tool_call) = self.element_to_tool_call(&element) {
                    tool_calls.push(tool_call);
                }
            } else {
                // Collect text content from non-tool elements
                text_content.push_str(&element.text_content);
                text_content.push(' ');
            }
        }

        let content = if text_content.trim().is_empty() {
            None
        } else {
            Some(text_content.trim().to_string())
        };

        Ok((content, tool_calls))
    }

    /// Convert XmlElement to ToolCall for backward compatibility
    fn element_to_tool_call(&self, element: &XmlElement) -> Option<ToolCall> {
        if element.tag != "tool_call" {
            return None;
        }

        let mut name = String::new();
        let mut id = String::new();
        let mut arguments = HashMap::new();

        // Extract name and id from children
        for child in &element.children {
            match child.tag.as_str() {
                "name" => name = child.text_content.clone(),
                "id" => id = child.text_content.clone(),
                "arguments" => {
                    // Extract arguments from children
                    for arg_child in &child.children {
                        arguments.insert(arg_child.tag.clone(), arg_child.text_content.clone());
                    }
                }
                _ => {}
            }
        }

        if name.is_empty() {
            return None;
        }

        if id.is_empty() {
            id = format!("tool_{}", shared::utils::snowflake::generate_id());
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

    /// Get current parsing state for debugging
    pub fn get_parsing_state(&self) -> String {
        format!(
            "Buffer size: {} bytes, Stack depth: {}, Completed: {}",
            self.buffer.len(),
            self.element_stack.len(),
            self.completed_elements.len()
        )
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
            id = format!("tool_{}", shared::utils::snowflake::generate_id());
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

    /// Parse general XML into a HashMap structure
    pub fn parse_xml(&self, xml: &str) -> Result<HashMap<String, Value>, shared::error::AppError> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut result = HashMap::new();
        let mut element_stack = Vec::new();
        let mut current_text = String::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let element_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    element_stack.push(element_name);
                    current_text.clear();
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    current_text.push_str(&text);
                }
                Ok(Event::End(ref e)) => {
                    let element_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if let Some(last_element) = element_stack.pop() {
                        if last_element == element_name {
                            // Store the text content for this element
                            if element_stack.is_empty() {
                                // Root element
                                let mut root_map = HashMap::new();
                                root_map.insert(
                                    element_name.clone(),
                                    Value::String(current_text.clone()),
                                );
                                result.insert(
                                    element_name,
                                    Value::Object(serde_json::Map::from_iter(
                                        root_map.into_iter().map(|(k, v)| (k, v)),
                                    )),
                                );
                            } else {
                                // Nested element - for simplicity, just store as string
                                result.insert(element_name, Value::String(current_text.clone()));
                            }
                        }
                    }
                    current_text.clear();
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(shared::error::AppError::Internal(format!(
                        "XML parsing error: {}",
                        e
                    )));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(result)
    }

    /// Advanced streaming XML parser that can handle any XML structure
    /// Returns completed elements as they become available
    pub fn parse_streaming_xml(
        &mut self,
        chunk: &str,
    ) -> Result<Vec<(String, HashMap<String, Value>)>, shared::error::AppError> {
        // Check buffer size limit
        if self.buffer.len() + chunk.len() > self.max_buffer_size {
            return Err(shared::error::AppError::Internal(format!(
                "XML parser buffer size limit exceeded: {} bytes",
                self.max_buffer_size
            )));
        }

        self.buffer.push_str(chunk);
        let mut completed_elements = Vec::new();

        // Use quick-xml's streaming parser
        let mut reader = Reader::from_str(&self.buffer);
        reader.config_mut().trim_text(true);

        let mut element_stack = Vec::new();
        let mut current_element = HashMap::new();
        let mut current_text = String::new();
        let mut buf = Vec::new();
        let mut processed_bytes = 0usize;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(ref e)) => {
                    let element_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    // If we have a current element, push it to stack
                    if !current_element.is_empty() {
                        element_stack.push((element_name.clone(), current_element.clone()));
                    }

                    current_element = HashMap::new();
                    current_element.insert("_tag".to_string(), Value::String(element_name.clone()));
                    current_text.clear();

                    // Store attributes if any
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
                            let value = String::from_utf8_lossy(&attr.value).to_string();
                            current_element.insert(format!("@{}", key), Value::String(value));
                        }
                    }
                }
                Ok(Event::Text(ref e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    current_text.push_str(&text);
                }
                Ok(Event::End(ref e)) => {
                    let element_name = String::from_utf8_lossy(e.name().as_ref()).to_string();

                    // Add text content if any
                    if !current_text.trim().is_empty() {
                        current_element
                            .insert("_text".to_string(), Value::String(current_text.clone()));
                    }

                    // If this is a complete top-level element, add it to results
                    if element_stack.is_empty() {
                        completed_elements.push((element_name.clone(), current_element.clone()));
                        processed_bytes = reader.buffer_position() as usize;
                    } else {
                        // Pop from stack and merge
                        if let Some((_parent_name, mut parent_element)) = element_stack.pop() {
                            parent_element.insert(
                                element_name,
                                Value::Object(current_element.into_iter().collect()),
                            );
                            current_element = parent_element;
                        }
                    }
                    current_text.clear();
                }
                Ok(Event::Eof) => break,
                Err(_) => {
                    // If we hit an error, it might be incomplete XML
                    // Keep the buffer as is and wait for more data
                    break;
                }
                _ => {}
            }
            buf.clear();
        }

        // Remove processed XML from buffer
        if processed_bytes > 0 && processed_bytes <= self.buffer.len() {
            self.buffer = self.buffer[processed_bytes..].to_string();
        }

        Ok(completed_elements)
    }
}
