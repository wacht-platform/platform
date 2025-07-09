use crate::template::{AgentTemplates, render_template};
use llm::LLMProvider;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use shared::commands::{Command, CreateMemoryCommand};
use shared::error::AppError;
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

#[derive(Deserialize, Serialize)]
struct MemoryEvaluationResponse {
    should_store: bool,
    reason: String,
    importance: f64,
    suggested_content: String,
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

        if !evaluation.should_store {
            // Log the reason for debugging
            tracing::debug!("Memory not stored. Reason: {}", evaluation.reason);
            return Ok(0); // Return dummy ID, memory not stored
        }

        // Use the LLM's suggested content and importance
        let final_content = evaluation.suggested_content;
        let mut importance = evaluation.importance.max(base_importance); // Use the higher of the two

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
            deployment_id: self.deployment_id,
            agent_id: self.agent_id,
            execution_context_id: Some(self.context_id), // Context-specific
            memory_type,
            content: final_content,
            embedding,
            importance,
        }
        .execute(&self.app_state)
        .await
        .map(|record| record.id)
    }

    /// Search memories with learning feedback

    /// Create a weak LLM for memory evaluation
    fn create_weak_llm(
        &self,
        system_prompt: Option<&str>,
    ) -> Result<Box<dyn LLMProvider>, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let mut builder = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.5-flash")
            .max_tokens(1000)
            .temperature(0.2); // Lower temperature for more consistent evaluation

        if let Some(system) = system_prompt {
            builder = builder.system(system);
        }

        builder
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to create LLM: {}", e)))
    }

    /// Evaluate if content is worth storing using LLM
    async fn evaluate_memory_worthiness(
        &self,
        content: &str,
        memory_type: &MemoryType,
        context: &HashMap<String, Value>,
    ) -> Result<MemoryEvaluationResponse, AppError> {
        // Get recent conversation topic if available
        let conversation_topic = context
            .get("conversation_topic")
            .and_then(|v| v.as_str())
            .unwrap_or("general task execution");

        let evaluation_context = json!({
            "memory_type": format!("{:?}", memory_type),
            "content": content,
            "context": serde_json::to_string(&context).unwrap_or_default(),
            "conversation_topic": conversation_topic,
        });

        let system_prompt = render_template(AgentTemplates::ACKNOWLEDGMENT, &evaluation_context)
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to render memory evaluation template: {}",
                    e
                ))
            })?;

        // Create LLM with system prompt
        let llm = self.create_weak_llm(Some(&system_prompt))?;

        // Simple user prompt
        let user_prompt = format!(
            "Content: {}\nMemory Type: {:?}\nConversation Topic: {}\nPlease evaluate if this content should be stored.",
            content, memory_type, conversation_topic
        );

        let messages = vec![ChatMessage::user().content(&user_prompt).build()];
        let response_text = {
            let response = llm
                .chat(&messages)
                .await
                .map_err(|e| AppError::Internal(format!("LLM memory evaluation failed: {}", e)))?;
            response.to_string()
        };

        // Extract JSON from potential markdown code blocks
        let json_str = if response_text.contains("```json") {
            // Extract content between ```json and ```
            let start = response_text.find("```json").map(|i| i + 7);
            let end = response_text.rfind("```");

            match (start, end) {
                (Some(s), Some(e)) if e > s => response_text[s..e].trim(),
                _ => response_text.trim(),
            }
        } else {
            response_text.trim()
        };

        // Parse the response
        serde_json::from_str(json_str).map_err(|e| {
            AppError::Internal(format!(
                "Failed to parse memory evaluation response: {}. Response: {}",
                e, response_text
            ))
        })
    }
}
