use crate::template::{AgentTemplates, render_template_with_prompt};
use crate::agentic::gemini_client::GeminiClient;
use serde_json::{Value, json};
use shared::commands::{Command, CreateMemoryCommand};
use shared::error::AppError;
use shared::dto::json::agent_memory::MemoryEvaluationResponse;
use shared::models::MemoryType;
use shared::state::AppState;
use std::collections::HashMap;

/// Enhanced memory manager with learning capabilities
pub struct MemoryManager {
    context_id: i64,
    agent_id: i64, // Keep for universal memories
    deployment_id: i64,
    app_state: AppState,
    working_memory: HashMap<String, WorkingMemoryItem>,
}

#[derive(Clone, Debug)]
struct WorkingMemoryItem {
    content: String,
    access_count: u32,
}


impl MemoryManager {
    pub fn new(context_id: i64, agent_id: i64, deployment_id: i64, app_state: AppState) -> Self {
        Self {
            context_id,
            agent_id,
            deployment_id,
            app_state,
            working_memory: HashMap::new(),
        }
    }

    /// Store a working memory item for the current task
    pub fn store_working_memory(&mut self, key: String, content: String) {
        self.working_memory.insert(
            key,
            WorkingMemoryItem {
                content,
                access_count: 0,
            },
        );
    }

    /// Get working memory item
    pub fn get_working_memory(&mut self, key: &str) -> Option<String> {
        if let Some(item) = self.working_memory.get_mut(key) {
            item.access_count += 1;
            Some(item.content.clone())
        } else {
            None
        }
    }

    /// Convert important working memory to long-term memory

    /// Store memory with contextual learning (context-specific)
    pub async fn store_contextual_memory(
        &self,
        content: String,
        memory_type: MemoryType,
        base_importance: f64,
        context: &HashMap<String, Value>,
    ) -> Result<i64, AppError> {
        // Use LLM to evaluate if content is worth storing
        let evaluation = self
            .evaluate_memory_worthiness(&content, &memory_type, context)
            .await?;

        if !evaluation.worth_storing {
            // Log the reason for debugging
            tracing::debug!("Memory not stored. Reason: {}", evaluation.reasoning);
            return Ok(0); // Return dummy ID, memory not stored
        }

        // Use the original content and map retention priority to importance
        let final_content = content.to_string();
        let mut importance = match evaluation.retention_priority.as_str() {
            "high" => 0.9,
            "medium" => 0.6,
            "low" => 0.3,
            _ => base_importance,
        }.max(base_importance); // Use the higher of the two

        // Additional importance adjustments based on context
        if let Some(Value::Bool(true)) = context.get("success") {
            importance = (importance + 0.1).min(1.0);
        }

        if let Some(Value::Bool(true)) = context.get("error_resolved") {
            importance = (importance + 0.15).min(1.0);
        }

        if let Some(Value::Number(fail_count)) = context.get("failure_count") {
            if let Some(fails) = fail_count.as_u64() {
                importance = (importance - (fails as f64 * 0.05)).max(0.1);
            }
        }

        // Generate embedding for the refined content
        let embedding = shared::commands::GenerateEmbeddingCommand::new(final_content.clone())
            .execute(&self.app_state)
            .await?;

        CreateMemoryCommand {
            id: self.app_state.sf.next_id()? as i64,
            content: final_content,
            embedding,
            memory_category: memory_type.as_str().to_string(),
            creation_context_id: Some(self.context_id),
            initial_importance: importance,
        }
        .execute(&self.app_state)
        .await
        .map(|record| record.id)
    }

    /// Search memories with learning feedback


    /// Evaluate if content is worth storing using LLM
    async fn evaluate_memory_worthiness(
        &self,
        content: &str,
        memory_type: &MemoryType,
        context: &HashMap<String, Value>,
    ) -> Result<MemoryEvaluationResponse, AppError> {
        let conversation_topic = context
            .get("conversation_topic")
            .and_then(|v| v.as_str())
            .unwrap_or("general task execution");

        let request_body = render_template_with_prompt(
            AgentTemplates::MEMORY_EVALUATION,
            json!({
                "conversation_history": Vec::<Value>::new(), // Empty for memory evaluation
                "memory_type": format!("{:?}", memory_type),
                "content": content,
                "context": serde_json::to_string(&context).unwrap_or_default(),
                "conversation_topic": conversation_topic,
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render memory evaluation template: {}",
                e
            ))
        })?;

        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;
        
        let client = GeminiClient::new(api_key, Some("gemini-2.0-flash-exp".to_string()));
        let evaluation = client
            .generate_structured_content::<MemoryEvaluationResponse>(request_body)
            .await?;

        Ok(evaluation)
    }
}
