// Compression module - Token management and conversation summarization

use super::core::AgentExecutor;
use crate::template::{render_template_with_prompt, AgentTemplates};

use commands::{Command, CreateConversationCommand, CreateMemoryCommand, GenerateEmbeddingsCommand};
use common::error::AppError;
use dto::json::agent_memory::MemoryCategory;
use models::{ConversationContent, ConversationMessageType};
use serde_json::json;

impl AgentExecutor {
    pub(super) async fn check_and_generate_summaries(&mut self) -> Result<(), AppError> {
        const TOKEN_THRESHOLD: usize = 100_000;
        const TARGET_TOKENS: usize = 80_000;

        let total_uncompressed_tokens: usize = self
            .conversations
            .iter()
            .filter(|conv| !matches!(conv.message_type, ConversationMessageType::ExecutionSummary))
            .map(|conv| conv.token_count as usize)
            .sum();

        if total_uncompressed_tokens >= TOKEN_THRESHOLD {
            let current_execution_start = self
                .conversations
                .iter()
                .rposition(|msg| matches!(msg.message_type, ConversationMessageType::UserMessage))
                .unwrap_or(self.conversations.len());

            let tokens_to_compress = total_uncompressed_tokens - TARGET_TOKENS;

            self.apply_sliding_window_compression(tokens_to_compress, current_execution_start)
                .await?;
        }

        Ok(())
    }

    async fn apply_sliding_window_compression(
        &mut self,
        tokens_to_compress: usize,
        current_execution_start: usize,
    ) -> Result<(), AppError> {
        tracing::info!(
            "Applying sliding window compression: need to compress {} tokens",
            tokens_to_compress
        );

        let mut executions: Vec<(usize, usize, String)> = Vec::new();
        let mut current_user_request = String::new();
        let mut execution_start = 0;

        for (idx, conv) in self.conversations.iter().enumerate() {
            if idx >= current_execution_start {
                break;
            }

            if matches!(conv.message_type, ConversationMessageType::UserMessage) {
                if idx > 0 {
                    executions.push((execution_start, idx, current_user_request.clone()));
                }

                execution_start = idx;
                if let ConversationContent::UserMessage { message, .. } = &conv.content {
                    current_user_request = message.clone();
                }
            }
        }

        if execution_start < current_execution_start && !current_user_request.is_empty() {
            executions.push((execution_start, current_execution_start, current_user_request));
        }

        let mut compressed_tokens = 0;

        for (exec_idx, (start_idx, end_idx, user_request)) in executions.iter().enumerate() {
            if compressed_tokens >= tokens_to_compress {
                break;
            }

            let already_summarized = self.conversations[*start_idx..*end_idx]
                .iter()
                .any(|msg| matches!(msg.message_type, ConversationMessageType::ExecutionSummary));

            if already_summarized {
                continue;
            }

            let execution_tokens: usize = self.conversations[*start_idx..*end_idx]
                .iter()
                .filter(|conv| !matches!(conv.message_type, ConversationMessageType::ExecutionSummary))
                .map(|conv| conv.token_count as usize)
                .sum();

            if execution_tokens < 100 {
                continue;
            }

            let execution_messages: Vec<_> = self.conversations[*start_idx..*end_idx]
                .iter()
                .filter_map(|msg| match serde_json::to_value(msg) {
                    Ok(_) => Some(json!({
                        "role": self.map_conversation_type_to_role(&msg.message_type),
                        "content": self.extract_conversation_content(&msg.content),
                    })),
                    Err(_) => None,
                })
                .collect();

            if execution_messages.is_empty() {
                continue;
            }

            match self.generate_execution_summary_for_messages(user_request.clone(), execution_messages).await {
                Ok(summary_tokens) => {
                    compressed_tokens += execution_tokens.saturating_sub(summary_tokens);
                }
                Err(e) => {
                    tracing::error!("Failed to generate summary for execution {}: {}", exec_idx, e);
                }
            }
        }

        Ok(())
    }

    async fn generate_execution_summary_for_messages(
        &mut self,
        user_request: String,
        execution_messages: Vec<serde_json::Value>,
    ) -> Result<usize, AppError> {
        use tiktoken_rs::cl100k_base;

        let existing_memories: Vec<String> = self.memories.iter().map(|m| m.content.clone()).collect();

        let request_body = render_template_with_prompt(
            AgentTemplates::EXECUTION_SUMMARY,
            json!({
                "user_request": user_request,
                "execution_messages": execution_messages,
                "existing_memories": existing_memories,
            }),
        )
        .map_err(|e| AppError::Internal(format!("Failed to render execution summary template: {e}")))?;

        let summary_response = self
            .create_weak_llm()?
            .generate_structured_content::<serde_json::Value>(request_body)
            .await
            .map_err(|e| AppError::Internal(format!("Summary generation failed: {e}")))?;

        let agent_execution = summary_response
            .get("agent_execution")
            .and_then(|v| v.as_str())
            .unwrap_or("Completed the requested task")
            .to_string();

        let memories = summary_response
            .get("memories")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        // Store extracted memories
        if !memories.is_empty() {
            self.store_extracted_memories(&memories).await;
        }

        let token_count = match cl100k_base() {
            Ok(bpe) => {
                let full_summary = format!("User: {user_request}\nAgent: {agent_execution}");
                bpe.encode_with_special_tokens(&full_summary).len()
            }
            Err(_) => {
                let full_summary = format!("User: {user_request}\nAgent: {agent_execution}");
                full_summary.len() / 4
            }
        };

        // Store summary conversation
        if let Ok(id) = self.app_state.sf.next_id() {
            let command = CreateConversationCommand::new(
                id as i64,
                self.context_id,
                ConversationContent::ExecutionSummary {
                    user_message: user_request,
                    agent_execution,
                    token_count,
                },
                ConversationMessageType::ExecutionSummary,
            );
            command.execute(&self.app_state).await?;
        }

        Ok(token_count)
    }

    async fn store_extracted_memories(&self, memories: &[serde_json::Value]) {
        let memory_contents: Vec<String> = memories
            .iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .map(String::from)
            .collect();

        let embeddings = match GenerateEmbeddingsCommand::new(memory_contents.clone())
            .with_task_type("RETRIEVAL_DOCUMENT".to_string())
            .execute(&self.app_state)
            .await
        {
            Ok(e) => e,
            Err(_) => return,
        };

        if embeddings.len() != memories.len() {
            return;
        }

        for (memory, embedding) in memories.iter().zip(embeddings.iter()) {
            if embedding.is_empty() {
                continue;
            }

            let content = memory.get("content").and_then(|c| c.as_str()).unwrap_or("");
            let category = memory
                .get("category")
                .and_then(|c| c.as_str())
                .and_then(MemoryCategory::from_str)
                .unwrap_or(MemoryCategory::Working);
            let importance = memory.get("importance").and_then(|i| i.as_f64()).unwrap_or(0.5);

            if let Ok(id) = self.app_state.sf.next_id() {
                let create_cmd = CreateMemoryCommand {
                    id: id as i64,
                    content: content.to_string(),
                    embedding: embedding.clone(),
                    memory_category: category,
                    creation_context_id: Some(self.context_id),
                    agent_id: Some(self.agent.id),
                    initial_importance: importance,
                };
                let _ = create_cmd.execute(&self.app_state).await;
            }
        }
    }
}
