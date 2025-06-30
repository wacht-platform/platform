use crate::error::AppError;
use pulldown_cmark::{Parser, html};

#[derive(Debug, Clone)]
pub struct TextProcessingService;

#[derive(Debug, Clone)]
pub struct TextChunk {
    pub content: String,
    pub chunk_index: usize,
}

impl TextProcessingService {
    pub fn new() -> Self {
        Self
    }

    pub fn extract_text_from_file(
        &self,
        file_content: &[u8],
        file_type: &str,
    ) -> Result<String, AppError> {
        let normalized_type = if file_type.contains("/") {
            file_type.split('/').last().unwrap_or(file_type)
        } else {
            file_type
        }
        .to_lowercase();

        match normalized_type.as_str() {
            "pdf" | "application/pdf" => self.extract_text_from_pdf(file_content),
            "txt" | "text" | "plain" | "text/plain" => self.extract_text_from_txt(file_content),
            "md" | "markdown" | "text/markdown" => self.extract_text_from_markdown(file_content),
            "json" | "application/json" => self.extract_text_from_json(file_content),
            _ => self.extract_text_from_txt(file_content),
        }
    }

    fn extract_text_from_pdf(&self, content: &[u8]) -> Result<String, AppError> {
        pdf_extract::extract_text_from_mem(content)
            .map_err(|e| AppError::Internal(format!("Failed to extract text from PDF: {}", e)))
    }

    fn extract_text_from_txt(&self, content: &[u8]) -> Result<String, AppError> {
        String::from_utf8(content.to_vec())
            .map_err(|e| AppError::Internal(format!("Failed to parse text file: {}", e)))
    }

    fn extract_text_from_markdown(&self, content: &[u8]) -> Result<String, AppError> {
        let markdown_content = String::from_utf8(content.to_vec())
            .map_err(|e| AppError::Internal(format!("Failed to parse markdown file: {}", e)))?;

        let parser = Parser::new(&markdown_content);
        let mut html_output = String::new();
        html::push_html(&mut html_output, parser);

        let text = html_output
            .replace("<br>", "\n")
            .replace("<br/>", "\n")
            .replace("<br />", "\n")
            .replace("</p>", "\n\n")
            .replace("</div>", "\n")
            .replace("</h1>", "\n\n")
            .replace("</h2>", "\n\n")
            .replace("</h3>", "\n\n")
            .replace("</h4>", "\n\n")
            .replace("</h5>", "\n\n")
            .replace("</h6>", "\n\n");

        let text = regex::Regex::new(r"<[^>]*>")
            .unwrap()
            .replace_all(&text, "")
            .to_string();

        Ok(text)
    }

    fn extract_text_from_html(&self, content: &[u8]) -> Result<String, AppError> {
        let html_content = String::from_utf8(content.to_vec())
            .map_err(|e| AppError::Internal(format!("Failed to parse HTML file: {}", e)))?;

        let text = html_content
            .replace("<br>", "\n")
            .replace("<br/>", "\n")
            .replace("<br />", "\n")
            .replace("</p>", "\n\n")
            .replace("</div>", "\n")
            .replace("</h1>", "\n\n")
            .replace("</h2>", "\n\n")
            .replace("</h3>", "\n\n")
            .replace("</h4>", "\n\n")
            .replace("</h5>", "\n\n")
            .replace("</h6>", "\n\n");

        // Remove HTML tags
        let text = regex::Regex::new(r"<[^>]*>")
            .unwrap()
            .replace_all(&text, "")
            .to_string();

        Ok(text)
    }

    fn extract_text_from_json(&self, content: &[u8]) -> Result<String, AppError> {
        let json_content = String::from_utf8(content.to_vec())
            .map_err(|e| AppError::Internal(format!("Failed to parse JSON file: {}", e)))?;

        match serde_json::from_str::<serde_json::Value>(&json_content) {
            Ok(json) => {
                let mut text_parts = Vec::new();
                self.extract_text_from_json_value(&json, &mut text_parts);
                Ok(text_parts.join(" "))
            }
            Err(_) => Ok(json_content),
        }
    }

    fn extract_text_from_json_value(
        &self,
        value: &serde_json::Value,
        text_parts: &mut Vec<String>,
    ) {
        match value {
            serde_json::Value::String(s) => text_parts.push(s.clone()),
            serde_json::Value::Array(arr) => {
                for item in arr {
                    self.extract_text_from_json_value(item, text_parts);
                }
            }
            serde_json::Value::Object(obj) => {
                for (_, v) in obj {
                    self.extract_text_from_json_value(v, text_parts);
                }
            }
            _ => {} // Skip numbers, booleans, null
        }
    }

    pub fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        _overlap: usize,
    ) -> Result<Vec<TextChunk>, AppError> {
        let mut chunks = Vec::new();
        let chars: Vec<char> = text.chars().collect();

        for (index, chunk_chars) in chars.chunks(chunk_size).enumerate() {
            let chunk_text = chunk_chars.iter().collect::<String>();

            chunks.push(TextChunk {
                content: chunk_text.clone(),
                chunk_index: index,
            });
        }

        Ok(chunks)
    }

    pub fn clean_text(&self, text: &str) -> String {
        // Remove excessive whitespace and normalize line endings
        let cleaned = regex::Regex::new(r"\s+")
            .unwrap()
            .replace_all(text.trim(), " ")
            .to_string();

        // Remove control characters except newlines and tabs
        cleaned
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect()
    }
}
