use super::AgentContext;
use shared::error::AppError;
use shared::state::AppState;
use serde_json::{json, Value};

pub struct ContextEngine {
    pub context: AgentContext,
    pub app_state: AppState,
}

impl ContextEngine {
    pub fn new(context: AgentContext, app_state: AppState) -> Self {
        Self { context, app_state }
    }

    pub async fn search(&self, query: &str) -> Result<Value, AppError> {
        let mut results = Vec::new();

        // Search across tools
        for tool in &self.context.tools {
            if self.matches_query(&tool.name, &tool.description, query) {
                results.push(json!({
                    "type": "tool",
                    "id": tool.id,
                    "name": tool.name,
                    "description": tool.description,
                    "tool_type": tool.tool_type,
                    "relevance_score": self.calculate_relevance(&tool.name, &tool.description, query)
                }));
            }
        }

        // Search across workflows
        for workflow in &self.context.workflows {
            if self.matches_query(&workflow.name, &workflow.description, query) {
                results.push(json!({
                    "type": "workflow",
                    "id": workflow.id,
                    "name": workflow.name,
                    "description": workflow.description,
                    "relevance_score": self.calculate_relevance(&workflow.name, &workflow.description, query)
                }));
            }
        }

        // Search across knowledge bases
        for kb in &self.context.knowledge_bases {
            if self.matches_query(&kb.name, &kb.description, query) {
                results.push(json!({
                    "type": "knowledge_base",
                    "id": kb.id,
                    "name": kb.name,
                    "description": kb.description,
                    "relevance_score": self.calculate_relevance(&kb.name, &kb.description, query)
                }));
            }
        }

        // Sort by relevance score (highest first)
        results.sort_by(|a, b| {
            let score_a = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        Ok(json!({
            "query": query,
            "results": results,
            "total_found": results.len(),
            "search_timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    pub async fn get_detailed_info(&self, resource_type: &str, resource_id: i64) -> Result<Value, AppError> {
        match resource_type {
            "tool" => {
                if let Some(tool) = self.context.tools.iter().find(|t| t.id == resource_id) {
                    Ok(json!({
                        "type": "tool",
                        "id": tool.id,
                        "name": tool.name,
                        "description": tool.description,
                        "tool_type": tool.tool_type,
                        "configuration": tool.configuration,
                        "created_at": tool.created_at,
                        "updated_at": tool.updated_at
                    }))
                } else {
                    Err(AppError::NotFound("Tool not found".to_string()))
                }
            }
            "workflow" => {
                if let Some(workflow) = self.context.workflows.iter().find(|w| w.id == resource_id) {
                    Ok(json!({
                        "type": "workflow",
                        "id": workflow.id,
                        "name": workflow.name,
                        "description": workflow.description,
                        "configuration": workflow.configuration,
                        "workflow_definition": workflow.workflow_definition,
                        "created_at": workflow.created_at,
                        "updated_at": workflow.updated_at
                    }))
                } else {
                    Err(AppError::NotFound("Workflow not found".to_string()))
                }
            }
            "knowledge_base" => {
                if let Some(kb) = self.context.knowledge_bases.iter().find(|k| k.id == resource_id) {
                    Ok(json!({
                        "type": "knowledge_base",
                        "id": kb.id,
                        "name": kb.name,
                        "description": kb.description,
                        "configuration": kb.configuration,
                        "created_at": kb.created_at,
                        "updated_at": kb.updated_at
                    }))
                } else {
                    Err(AppError::NotFound("Knowledge base not found".to_string()))
                }
            }
            _ => Err(AppError::BadRequest(format!("Unknown resource type: {}", resource_type)))
        }
    }

    fn matches_query(&self, name: &str, description: &Option<String>, query: &str) -> bool {
        let query_lower = query.to_lowercase();
        let name_lower = name.to_lowercase();
        
        // Check if query matches name
        if name_lower.contains(&query_lower) {
            return true;
        }

        // Check if query matches description
        if let Some(desc) = description {
            if desc.to_lowercase().contains(&query_lower) {
                return true;
            }
        }

        // Check for keyword matches
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let name_words: Vec<&str> = name_lower.split_whitespace().collect();
        
        for query_word in &query_words {
            for name_word in &name_words {
                if name_word.contains(query_word) || query_word.contains(name_word) {
                    return true;
                }
            }
        }

        false
    }

    fn calculate_relevance(&self, name: &str, description: &Option<String>, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let name_lower = name.to_lowercase();
        let mut score = 0.0;

        // Exact name match gets highest score
        if name_lower == query_lower {
            score += 100.0;
        } else if name_lower.contains(&query_lower) {
            score += 50.0;
        }

        // Description matches get lower scores
        if let Some(desc) = description {
            let desc_lower = desc.to_lowercase();
            if desc_lower.contains(&query_lower) {
                score += 25.0;
            }
        }

        // Word-level matching
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let name_words: Vec<&str> = name_lower.split_whitespace().collect();
        
        for query_word in &query_words {
            for name_word in &name_words {
                if name_word == query_word {
                    score += 10.0;
                } else if name_word.contains(query_word) || query_word.contains(name_word) {
                    score += 5.0;
                }
            }
        }

        score
    }

    pub async fn store_context(&self, key: &str, _data: &Value) -> Result<Value, AppError> {
        // For now, we'll use Redis to store context data
        // This is a placeholder implementation
        let redis_key = format!("agent_context:{}:{}", self.context.agent_id, key);
        
        // Store in Redis (placeholder - would need actual Redis implementation)
        Ok(json!({
            "key": key,
            "stored": true,
            "redis_key": redis_key,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    pub async fn fetch_context(&self, key: &str) -> Result<Value, AppError> {
        // For now, we'll use Redis to fetch context data
        // This is a placeholder implementation
        let redis_key = format!("agent_context:{}:{}", self.context.agent_id, key);
        
        // Fetch from Redis (placeholder - would need actual Redis implementation)
        Ok(json!({
            "key": key,
            "data": null,
            "redis_key": redis_key,
            "found": false,
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }
}
