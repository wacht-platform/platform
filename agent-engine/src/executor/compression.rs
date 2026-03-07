// Compression module - Token management and conversation summarization

use super::core::AgentExecutor;
use crate::template::{render_template_with_prompt, AgentTemplates};

use commands::{CreateConversationCommand, CreateMemoryCommand, GenerateEmbeddingsCommand};
use common::error::AppError;
use dto::json::agent_memory::MemoryCategory;
use models::{ActionResult, ConversationContent, ConversationMessageType, ConversationRecord};
use serde_json::{json, Value};

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
            executions.push((
                execution_start,
                current_execution_start,
                current_user_request,
            ));
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
                .filter(|conv| {
                    !matches!(conv.message_type, ConversationMessageType::ExecutionSummary)
                })
                .map(|conv| conv.token_count as usize)
                .sum();

            if execution_tokens < 100 {
                continue;
            }

            let execution_messages: Vec<_> = self.conversations[*start_idx..*end_idx]
                .iter()
                .filter_map(|msg| {
                    let compact_content = self.compact_execution_message(msg);
                    if compact_content.is_empty() {
                        return None;
                    }

                    Some(json!({
                        "role": self.map_conversation_type_to_role(&msg.message_type),
                        "message_type": conversation_message_type_label(&msg.message_type),
                        "timestamp": msg.created_at.to_rfc3339(),
                        "content": compact_content,
                    }))
                })
                .collect();

            if execution_messages.is_empty() {
                continue;
            }

            match self
                .generate_execution_summary_for_messages(user_request.clone(), execution_messages)
                .await
            {
                Ok(summary_tokens) => {
                    compressed_tokens += execution_tokens.saturating_sub(summary_tokens);
                }
                Err(e) => {
                    tracing::error!(
                        "Failed to generate summary for execution {}: {}",
                        exec_idx,
                        e
                    );
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

        let existing_memories: Vec<String> =
            self.memories.iter().map(|m| m.content.clone()).collect();

        let request_body = render_template_with_prompt(
            AgentTemplates::EXECUTION_SUMMARY,
            json!({
                "user_request": user_request,
                "execution_messages": execution_messages,
                "existing_memories": existing_memories,
            }),
        )
        .map_err(|e| {
            AppError::Internal(format!("Failed to render execution summary template: {e}"))
        })?;

        let (summary_response, _) = self
            .create_weak_llm()
            .await?
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
        if let Ok(id) = self.ctx.app_state.sf.next_id() {
            let command = CreateConversationCommand::new(
                id as i64,
                self.ctx.context_id,
                ConversationContent::ExecutionSummary {
                    user_message: user_request,
                    agent_execution,
                    token_count,
                },
                ConversationMessageType::ExecutionSummary,
            );
            command
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?;
        }

        Ok(token_count)
    }

    async fn store_extracted_memories(&self, memories: &[serde_json::Value]) {
        let memory_contents: Vec<String> = memories
            .iter()
            .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
            .map(String::from)
            .collect();

        let gemini_api_key = match std::env::var("GEMINI_API_KEY") {
            Ok(value) => value,
            Err(_) => return,
        };
        let gemini_model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "models/gemini-embedding-001".to_string());
        let gemini_client = reqwest::Client::new();

        let embeddings = match GenerateEmbeddingsCommand::new(memory_contents.clone())
            .with_task_type("RETRIEVAL_DOCUMENT".to_string())
            .execute_with_deps(commands::EmbeddingApiDeps {
                client: &gemini_client,
                api_key: &gemini_api_key,
                model: &gemini_model,
            })
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
            let importance = memory
                .get("importance")
                .and_then(|i| i.as_f64())
                .unwrap_or(0.5);

            if let Ok(id) = self.ctx.app_state.sf.next_id() {
                let create_cmd = CreateMemoryCommand {
                    id: id as i64,
                    content: content.to_string(),
                    embedding: embedding.clone(),
                    memory_category: category,
                    creation_context_id: Some(self.ctx.context_id),
                    agent_id: Some(self.ctx.agent.id),
                    initial_importance: importance,
                };
                let _ = create_cmd
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await;
            }
        }
    }

    fn compact_execution_message(&self, message: &ConversationRecord) -> String {
        match &message.content {
            ConversationContent::UserMessage { message, .. } => {
                format!("USER {}", truncate_for_summary(message, 240))
            }
            ConversationContent::AssistantAcknowledgment {
                acknowledgment_message,
                ..
            } => format!("ACK {}", truncate_for_summary(acknowledgment_message, 220)),
            ConversationContent::AgentResponse { response, .. } => {
                format!("RESP {}", truncate_for_summary(response, 320))
            }
            ConversationContent::UserInputRequest {
                question, context, ..
            } => format!(
                "ASK question={} context={}",
                truncate_for_summary(question, 180),
                truncate_for_summary(context, 140)
            ),
            ConversationContent::SystemDecision {
                step,
                reasoning,
                confidence,
                ..
            } => format!(
                "DECISION step={} confidence={:.2} reasoning={}",
                step,
                confidence,
                truncate_for_summary(reasoning, 220)
            ),
            ConversationContent::ActionExecutionResult {
                task_execution,
                execution_status,
                blocking_reason,
            } => {
                let action_count = task_execution.actions.actions.len();
                let result_items = task_execution
                    .actual_result
                    .as_ref()
                    .map(|results| {
                        results
                            .iter()
                            .map(compact_action_result)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_else(|| "no_results".to_string());

                let blocking = blocking_reason
                    .as_deref()
                    .map(|reason| format!(" blocking={}", truncate_for_summary(reason, 140)))
                    .unwrap_or_default();

                format!(
                    "ACTION status={:?} actions={} approach={} results=[{}]{}",
                    execution_status,
                    action_count,
                    truncate_for_summary(&task_execution.approach, 120),
                    result_items,
                    blocking
                )
            }
            ConversationContent::ContextResults {
                query,
                result_count,
                results,
                ..
            } => format!(
                "CONTEXT query={} count={} preview={}",
                truncate_for_summary(query, 120),
                result_count,
                truncate_for_summary(&compact_json_preview(results, 220), 220)
            ),
            ConversationContent::ExecutionSummary {
                agent_execution, ..
            } => format!("SUMMARY {}", truncate_for_summary(agent_execution, 320)),
            ConversationContent::PlatformFunctionResult {
                execution_id,
                result,
            } => format!(
                "PLATFORM execution_id={} result={}",
                execution_id,
                truncate_for_summary(result, 220)
            ),
        }
    }
}

fn compact_action_result(result: &ActionResult) -> String {
    let tool_name = result
        .result
        .as_ref()
        .and_then(|v| v.get("tool_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown_tool");

    let output = result.result.as_ref();
    let output_status = output
        .and_then(|v| v.get("status"))
        .and_then(|v| v.as_str())
        .unwrap_or(match result.status {
            models::ActionResultStatus::Success => "success",
            models::ActionResultStatus::Error => "error",
        });
    let saved_output_path = output
        .and_then(|v| v.get("meta"))
        .and_then(|v| v.get("saved_output_path"))
        .and_then(|v| v.as_str());
    let preview = output
        .and_then(|v| v.get("data"))
        .map(|data| compact_json_preview(data, 160))
        .unwrap_or_else(|| {
            output
                .map(|data| compact_json_preview(data, 160))
                .unwrap_or_else(|| "no_output".to_string())
        });

    match saved_output_path {
        Some(path) => format!(
            "{}:{} preview={} saved={}",
            tool_name,
            output_status,
            truncate_for_summary(&preview, 160),
            path
        ),
        None => format!(
            "{}:{} preview={}",
            tool_name,
            output_status,
            truncate_for_summary(&preview, 160)
        ),
    }
}

fn compact_json_preview(value: &Value, limit: usize) -> String {
    let raw = serde_json::to_string(value).unwrap_or_else(|_| "<unserializable>".to_string());
    truncate_for_summary(&raw, limit)
}

fn truncate_for_summary(input: &str, limit: usize) -> String {
    let normalized = input.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut truncated = normalized.chars().take(limit).collect::<String>();
    if normalized.chars().count() > limit {
        truncated.push_str("...");
    }
    truncated
}

fn conversation_message_type_label(message_type: &ConversationMessageType) -> &'static str {
    match message_type {
        ConversationMessageType::UserMessage => "user_message",
        ConversationMessageType::AgentResponse => "agent_response",
        ConversationMessageType::AssistantAcknowledgment => "assistant_acknowledgment",
        ConversationMessageType::ActionExecutionResult => "action_execution_result",
        ConversationMessageType::SystemDecision => "system_decision",
        ConversationMessageType::ContextResults => "context_results",
        ConversationMessageType::UserInputRequest => "user_input_request",
        ConversationMessageType::ExecutionSummary => "execution_summary",
        ConversationMessageType::PlatformFunctionResult => "platform_function_result",
    }
}
