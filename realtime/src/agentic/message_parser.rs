#[derive(Debug)]
pub struct MessageParser {
    buffer: String,
    inside_message_tag: bool,
    message_start_pos: Option<usize>,
    sent_length: usize,
    complete: bool,
}

impl MessageParser {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            inside_message_tag: false,
            message_start_pos: None,
            sent_length: 0,
            complete: false,
        }
    }

    pub fn parse(&mut self, chunk: &str) -> Option<String> {
        if chunk.is_empty() || self.complete {
            return None;
        }

        self.buffer.push_str(chunk);

        if !self.inside_message_tag {
            if let Some(start_pos) = self.find_message_start() {
                self.inside_message_tag = true;
                self.message_start_pos = Some(start_pos);
                self.sent_length = 0;
            }
        }

        if self.inside_message_tag {
            if let Some(end_pos) = self.find_message_end() {
                // Found closing tag - return any remaining content and mark as complete
                let start_pos = self.message_start_pos.unwrap();
                let full_content = self.buffer[start_pos..end_pos].to_string();

                let result = if full_content.len() > self.sent_length {
                    Some(full_content[self.sent_length..].to_string())
                } else {
                    None
                };

                self.reset(); // Reset for next message
                self.compact_buffer_if_needed();
                return result;
            } else {
                // Still inside message tag - return any new content
                let start_pos = self.message_start_pos.unwrap();
                let current_content = &self.buffer[start_pos..];

                if current_content.len() > self.sent_length {
                    let new_content = current_content[self.sent_length..].to_string();
                    self.sent_length = current_content.len();
                    return Some(new_content);
                }
            }
        }

        self.compact_buffer_if_needed();
        None
    }

    fn find_message_start(&self) -> Option<usize> {
        if let Some(tag_start) = self.buffer.find("<message>") {
            return Some(tag_start + "<message>".len());
        }
        None
    }

    fn find_message_end(&self) -> Option<usize> {
        if let Some(start_pos) = self.message_start_pos {
            if let Some(relative_pos) = self.buffer[start_pos..].find("</message>") {
                return Some(start_pos + relative_pos);
            }
        }
        None
    }

    fn reset(&mut self) {
        self.inside_message_tag = false;
        self.message_start_pos = None;
        self.sent_length = 0;
        self.complete = true;
        // Keep buffer in case there are more messages, but clean up processed content
        if let Some(end_pos) = self.find_last_message_end() {
            self.buffer.drain(0..end_pos + "</message>".len());
        }
    }

    fn find_last_message_end(&self) -> Option<usize> {
        self.buffer.rfind("</message>")
    }

    fn compact_buffer_if_needed(&mut self) {
        if self.complete && self.buffer.len() > 1024 {
            self.buffer.clear();
        }
    }
}
