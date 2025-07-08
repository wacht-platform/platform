use shared::error::AppError;
use shared::models::{EnhancedCitation, CitationType as ModelCitationType, UsageType};
use crate::agentic::xml_parser;

/// Extracts citations from LLM responses
pub struct CitationExtractor;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ResponseWithCitations {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub citations: Citations,
    #[serde(default)]
    pub response: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct Citations {
    #[serde(default)]
    pub memory_reference: Vec<MemoryReference>,
    #[serde(default)]
    pub conversation_reference: Vec<ConversationReference>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MemoryReference {
    pub id: String,
    pub relevance: String,
    pub usefulness: String,
    pub confidence: String,
    pub usage_type: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ConversationReference {
    pub id: String,
    pub relevance: String,
    pub usefulness: String,
    pub confidence: String,
    pub usage_type: String,
    #[serde(default)]
    pub description: String,
}

impl CitationExtractor {
    /// Extract citations from an XML response
    pub fn extract_citations(xml_response: &str) -> Result<Vec<EnhancedCitation>, AppError> {
        // Try to parse the full response
        let response: ResponseWithCitations = match xml_parser::from_str(xml_response) {
            Ok(r) => r,
            Err(_) => {
                // If full parsing fails, try to extract just citations section
                if let Some(citations_xml) = Self::extract_citations_section(xml_response) {
                    match xml_parser::from_str::<Citations>(&citations_xml) {
                        Ok(citations) => ResponseWithCitations {
                            citations,
                            ..Default::default()
                        },
                        Err(_) => return Ok(Vec::new()), // No citations found
                    }
                } else {
                    return Ok(Vec::new()); // No citations section
                }
            }
        };

        let mut citations = Vec::new();

        // Convert memory references
        for mem_ref in response.citations.memory_reference {
            if let Ok(id) = mem_ref.id.parse::<i64>() {
                citations.push(EnhancedCitation {
                    item_id: id,
                    item_type: ModelCitationType::Memory,
                    relevance_score: Self::parse_level(&mem_ref.relevance),
                    usefulness_score: Self::parse_level(&mem_ref.usefulness),
                    confidence: Self::parse_level(&mem_ref.confidence),
                    usage_type: Self::parse_usage_type(&mem_ref.usage_type),
                    description: Some(mem_ref.description),
                });
            }
        }

        // Convert conversation references
        for conv_ref in response.citations.conversation_reference {
            if let Ok(id) = conv_ref.id.parse::<i64>() {
                citations.push(EnhancedCitation {
                    item_id: id,
                    item_type: ModelCitationType::Conversation,
                    relevance_score: Self::parse_level(&conv_ref.relevance),
                    usefulness_score: Self::parse_level(&conv_ref.usefulness),
                    confidence: Self::parse_level(&conv_ref.confidence),
                    usage_type: Self::parse_usage_type(&conv_ref.usage_type),
                    description: Some(conv_ref.description),
                });
            }
        }

        Ok(citations)
    }

    /// Extract reasoning from response
    pub fn extract_reasoning(xml_response: &str) -> Option<String> {
        // Try full parse first
        if let Ok(response) = xml_parser::from_str::<ResponseWithCitations>(xml_response) {
            if !response.reasoning.is_empty() {
                return Some(response.reasoning);
            }
        }

        // Fallback: extract reasoning tag manually
        if let Some(start) = xml_response.find("<reasoning>") {
            if let Some(end) = xml_response.find("</reasoning>") {
                let reasoning = xml_response[start + 11..end].trim().to_string();
                if !reasoning.is_empty() {
                    return Some(reasoning);
                }
            }
        }

        None
    }

    /// Extract the main response content
    pub fn extract_response_content(xml_response: &str) -> String {
        // Try full parse first
        if let Ok(response) = xml_parser::from_str::<ResponseWithCitations>(xml_response) {
            if !response.message.is_empty() {
                return response.message;
            }
            if let Some(resp) = response.response {
                if !resp.is_empty() {
                    return resp;
                }
            }
        }

        // Fallback: extract message tag
        if let Some(start) = xml_response.find("<message>") {
            if let Some(end) = xml_response.find("</message>") {
                return xml_response[start + 9..end].trim().to_string();
            }
        }

        xml_response.to_string()
    }

    fn extract_citations_section(xml: &str) -> Option<String> {
        if let Some(start) = xml.find("<citations>") {
            if let Some(end) = xml.find("</citations>") {
                return Some(xml[start..end + 12].to_string());
            }
        }
        None
    }

    fn parse_level(level: &str) -> f64 {
        match level.to_lowercase().as_str() {
            "high" => 0.9,
            "medium" => 0.6,
            "low" => 0.3,
            _ => 0.5,
        }
    }

    fn parse_usage_type(usage: &str) -> UsageType {
        match usage.to_lowercase().replace('_', " ").as_str() {
            "direct quote" => UsageType::DirectQuote,
            "paraphrase" => UsageType::Paraphrase,
            "inspiration" => UsageType::Inspiration,
            "background" => UsageType::Background,
            _ => UsageType::Background,
        }
    }
}