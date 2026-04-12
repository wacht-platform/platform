use crate::error::AppError;
use lopdf::{Document, Object, ObjectId};
use pulldown_cmark::{Parser, html};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::BTreeMap;
use tiktoken_rs::cl100k_base;

#[derive(Clone)]
pub struct TextProcessingService;

#[derive(Clone)]
pub struct TextChunk {
    pub content: String,
    pub chunk_index: usize,
}

#[derive(Clone)]
pub struct PdfPageChunk {
    pub content: Vec<u8>,
    pub start_page: u32,
    pub end_page: u32,
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
            "xml" | "application/xml" | "text/xml" => self.extract_text_from_xml(file_content),
            _ => self.extract_text_from_txt(file_content),
        }
    }

    fn extract_text_from_pdf(&self, content: &[u8]) -> Result<String, AppError> {
        pdf_extract::extract_text_from_mem(content)
            .map_err(|e| AppError::Internal(format!("Failed to extract text from PDF: {}", e)))
    }

    pub fn split_pdf_into_page_groups(
        &self,
        content: &[u8],
        pages_per_chunk: usize,
    ) -> Result<Vec<PdfPageChunk>, AppError> {
        if pages_per_chunk == 0 {
            return Err(AppError::Internal(
                "pages_per_chunk must be greater than zero".to_string(),
            ));
        }

        let document = Document::load_mem(content)
            .map_err(|e| AppError::Internal(format!("Failed to load PDF: {}", e)))?;
        let pages = document.get_pages();
        let page_numbers = pages.keys().copied().collect::<Vec<_>>();

        if page_numbers.is_empty() {
            return Ok(Vec::new());
        }

        let mut chunks = Vec::new();
        for page_group in page_numbers.chunks(pages_per_chunk) {
            let mut subset = build_pdf_subset(&document, page_group)?;
            let mut bytes = Vec::new();
            subset
                .save_to(&mut bytes)
                .map_err(|e| AppError::Internal(format!("Failed to save PDF subset: {}", e)))?;

            chunks.push(PdfPageChunk {
                content: bytes,
                start_page: *page_group
                    .first()
                    .ok_or_else(|| AppError::Internal("Missing start page".to_string()))?,
                end_page: *page_group
                    .last()
                    .ok_or_else(|| AppError::Internal("Missing end page".to_string()))?,
            });
        }

        Ok(chunks)
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

    fn extract_text_from_xml(&self, content: &[u8]) -> Result<String, AppError> {
        let mut reader = Reader::from_reader(content);
        reader.config_mut().trim_text(true);

        let mut text_parts = Vec::new();
        let mut buf = Vec::new();

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    // Extract attribute values
                    for attr in e.attributes() {
                        if let Ok(attr) = attr {
                            if let Ok(value) = std::str::from_utf8(&attr.value) {
                                if !value.trim().is_empty() {
                                    text_parts.push(value.trim().to_string());
                                }
                            }
                        }
                    }
                }
                Ok(Event::Text(e)) => {
                    if let Ok(text) = e.unescape() {
                        let text = text.trim();
                        if !text.is_empty() {
                            text_parts.push(text.to_string());
                        }
                    }
                }
                Ok(Event::CData(e)) => {
                    if let Ok(text) = std::str::from_utf8(&e) {
                        let text = text.trim();
                        if !text.is_empty() {
                            text_parts.push(text.to_string());
                        }
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(AppError::Internal(format!("Error parsing XML: {}", e)));
                }
                _ => {}
            }
            buf.clear();
        }

        Ok(text_parts.join(" "))
    }

    pub fn chunk_text(
        &self,
        text: &str,
        chunk_size: usize,
        overlap: usize,
    ) -> Result<Vec<TextChunk>, AppError> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }

        if chunk_size == 0 {
            return Err(AppError::Internal(
                "chunk_size must be greater than zero".to_string(),
            ));
        }

        let bpe = cl100k_base()
            .map_err(|e| AppError::Internal(format!("Failed to initialize tokenizer: {}", e)))?;
        let tokens = bpe
            .split_by_token_ordinary(text)
            .map_err(|e| AppError::Internal(format!("Failed to tokenize text: {}", e)))?;

        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let step = chunk_size.saturating_sub(overlap).max(1);
        let mut chunks = Vec::new();
        let mut start = 0usize;
        let mut index = 0usize;

        while start < tokens.len() {
            let end = (start + chunk_size).min(tokens.len());
            let chunk_text = tokens[start..end].concat().trim().to_string();

            if !chunk_text.is_empty() {
                chunks.push(TextChunk {
                    content: chunk_text,
                    chunk_index: index,
                });
                index += 1;
            }

            if end == tokens.len() {
                break;
            }

            start += step;
        }

        Ok(chunks)
    }

    pub fn clean_text(&self, text: &str) -> String {
        let cleaned = regex::Regex::new(r"\s+")
            .unwrap()
            .replace_all(text.trim(), " ")
            .to_string();

        cleaned
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect()
    }
}

fn build_pdf_subset(source: &Document, page_numbers: &[u32]) -> Result<Document, AppError> {
    let source_pages = source.get_pages();
    let selected_pages = page_numbers
        .iter()
        .filter_map(|page_number| source_pages.get(page_number).copied())
        .collect::<Vec<_>>();

    if selected_pages.is_empty() {
        return Err(AppError::Internal(
            "No PDF pages selected for subset".to_string(),
        ));
    }

    let mut document = Document::with_version(source.version.as_str());
    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;
    let selected_set = selected_pages
        .iter()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    let mut selected_page_objects = BTreeMap::new();

    for (object_id, object) in source.objects.iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog_object = Some((
                    catalog_object.map(|(id, _)| id).unwrap_or(*object_id),
                    object.clone(),
                ));
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref existing)) = pages_object {
                        if let Ok(existing_dictionary) = existing.as_dict() {
                            dictionary.extend(existing_dictionary);
                        }
                    }
                    pages_object = Some((
                        pages_object.map(|(id, _)| id).unwrap_or(*object_id),
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            b"Page" if selected_set.contains(object_id) => {
                selected_page_objects.insert(*object_id, object.clone());
            }
            b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                document.objects.insert(*object_id, object.clone());
            }
        }
    }

    let catalog_object = catalog_object
        .ok_or_else(|| AppError::Internal("Catalog root not found in PDF".to_string()))?;
    let pages_object = pages_object
        .ok_or_else(|| AppError::Internal("Pages root not found in PDF".to_string()))?;

    for (object_id, object) in selected_page_objects.iter() {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_object.0);
            document
                .objects
                .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    if let Ok(dictionary) = pages_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", selected_page_objects.len() as u32);
        dictionary.set(
            "Kids",
            selected_page_objects
                .keys()
                .copied()
                .map(Object::Reference)
                .collect::<Vec<_>>(),
        );
        document
            .objects
            .insert(pages_object.0, Object::Dictionary(dictionary));
    }

    if let Ok(dictionary) = catalog_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_object.0);
        dictionary.remove(b"Outlines");
        document
            .objects
            .insert(catalog_object.0, Object::Dictionary(dictionary));
    }

    document.trailer.set("Root", catalog_object.0);
    document.max_id = document
        .objects
        .keys()
        .map(|(object_id, _)| *object_id)
        .max()
        .unwrap_or(1);
    document.renumber_objects();
    document.compress();

    Ok(document)
}
