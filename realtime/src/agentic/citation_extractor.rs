use shared::error::AppError;
use shared::models::{EnhancedCitation, CitationType as ModelCitationType, UsageType};
use crate::agentic::json_parser;
use serde_json::Value;

/// Extracts citations from LLM responses
pub struct CitationExtractor;

impl CitationExtractor {
    /// Extract citations from JSON response
    pub fn extract_citations(json_response: &str) -> Result<Vec<EnhancedCitation>, AppError> {
        let value: Value = json_parser::from_str(json_response)?;
        let mut citations = Vec::new();
        
        // Look for citations in various possible locations
        let citations_value = value.get("citations")
            .or_else(|| value.get("execution_plan").and_then(|ep| ep.get("citations")))
            .or_else(|| value.get("acknowledgment").and_then(|ack| ack.get("citations")))
            .or_else(|| value.get("task_exploration").and_then(|te| te.get("citations")))
            .or_else(|| value.get("task_verification").and_then(|tv| tv.get("citations")))
            .or_else(|| value.get("task_correction").and_then(|tc| tc.get("citations")));
            
        if let Some(citations_obj) = citations_value {
            // Process memory references
            if let Some(mem_refs) = citations_obj.get("memory_references") {
                if let Some(refs_array) = mem_refs.as_array() {
                    for mem_ref in refs_array {
                        if let Some(id_str) = mem_ref.get("id").and_then(|v| v.as_str()) {
                            if let Ok(id) = id_str.parse::<i64>() {
                                citations.push(EnhancedCitation {
                                    item_id: id,
                                    item_type: ModelCitationType::Memory,
                                    relevance_score: Self::parse_level(
                                        mem_ref.get("relevance").and_then(|v| v.as_str()).unwrap_or("medium")
                                    ),
                                    usefulness_score: Self::parse_level(
                                        mem_ref.get("usefulness").and_then(|v| v.as_str()).unwrap_or("medium")
                                    ),
                                    confidence: Self::parse_level(
                                        mem_ref.get("confidence").and_then(|v| v.as_str()).unwrap_or("medium")
                                    ),
                                    usage_type: Self::parse_usage_type(
                                        mem_ref.get("usage_type").and_then(|v| v.as_str()).unwrap_or("background")
                                    ),
                                    description: mem_ref.get("description")
                                        .or_else(|| mem_ref.get("content"))
                                        .and_then(|v| v.as_str())
                                        .map(|s| s.to_string()),
                                });
                            }
                        }
                    }
                }
            }
            
            // Conversations no longer support citations
        }
        
        Ok(citations)
    }

    /// Extract reasoning from JSON response
    pub fn extract_reasoning(response: &str) -> Option<String> {
        if let Ok(value) = json_parser::from_str::<Value>(response) {
            // Check various possible reasoning locations
            let reasoning_checks = [
                value.get("reasoning"),
                value.get("acknowledgment").and_then(|ack| ack.get("reasoning")),
                value.get("task_exploration").and_then(|te| te.get("analysis")),
                value.get("task_verification").and_then(|tv| tv.get("results_summary")),
            ];
            
            for check in &reasoning_checks {
                if let Some(reasoning) = check.and_then(|v| v.as_str()) {
                    if !reasoning.is_empty() {
                        return Some(reasoning.to_string());
                    }
                }
            }
        }
        None
    }

    /// Extract the main response content from JSON
    pub fn extract_response_content(response: &str) -> String {
        if let Ok(value) = json_parser::from_str::<Value>(response) {
            // Check various possible message locations
            let message_checks = [
                value.get("message"),
                value.get("execution_plan").and_then(|ep| ep.get("message")),
                value.get("acknowledgment").and_then(|ack| ack.get("message")),
                value.get("task_exploration").and_then(|te| te.get("message")),
                value.get("task_verification").and_then(|tv| tv.get("message")),
                value.get("task_correction").and_then(|tc| tc.get("message")),
            ];
            
            for check in &message_checks {
                if let Some(msg) = check.and_then(|v| v.as_str()) {
                    if !msg.is_empty() {
                        return msg.to_string();
                    }
                }
            }
        }
        
        response.to_string()
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