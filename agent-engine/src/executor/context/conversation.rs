use super::core::AgentExecutor;
use crate::executor::runtime::step_control::{
    DATABASE_ERROR_RETRY_STEP, LLM_REQUEST_FAILED_STEP, RETRYABLE_EXECUTION_ERROR_STEP,
    STRUCTURED_OUTPUT_TRUNCATED_STEP, TOOL_LOAD_REQUIRED_STEP,
};
use crate::llm::{
    SemanticLlmContentBlock, SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest,
};
use crate::template::{render_prompt_text, render_template_json, AgentTemplates};

use commands::{CreateConversationCommand, DispatchConversationCleanupTaskCommand};
use common::error::AppError;
use dto::json::{LlmHistoryEntry, LlmHistoryPart, StreamEvent};
use models::{ConversationContent, ConversationMessageType, ConversationRecord};
use queries::GetCompactionWindowConversationsQuery;
use serde_json::{json, Value};

impl AgentExecutor {
    pub(crate) fn llm_history_entry_text(entry: &LlmHistoryEntry) -> String {
        let body = if !entry.parts.is_empty() {
            entry.parts
                .iter()
                .filter_map(|part| {
                    part.text.as_ref().cloned().or_else(|| {
                        part.inline_data
                            .as_ref()
                            .map(|data| format!("[inline data: {}]", data.mime_type))
                    })
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            entry.content.clone().unwrap_or_default()
        };

        let trimmed = body.trim();
        if entry.parts.is_empty() {
            if let Some(timestamp) = entry.timestamp.as_ref() {
                if !trimmed.is_empty() {
                    return format!("[{timestamp}] {trimmed}");
                }
            }
        }

        trimmed.to_string()
    }

    pub(crate) fn semantic_message_from_history_entry(entry: &LlmHistoryEntry) -> SemanticLlmMessage {
        if !entry.parts.is_empty() {
            let mut content_blocks = Vec::new();
            for part in &entry.parts {
                if let Some(text) = part.text.as_ref() {
                    content_blocks.push(SemanticLlmContentBlock::Text { text: text.clone() });
                }
                if let Some(data) = part.inline_data.as_ref() {
                    content_blocks.push(SemanticLlmContentBlock::InlineData {
                        mime_type: data.mime_type.clone(),
                        data: data.data.clone(),
                    });
                }
            }
            return SemanticLlmMessage {
                role: entry.role.clone(),
                content_blocks,
            };
        }

        SemanticLlmMessage::text(entry.role.clone(), Self::llm_history_entry_text(entry))
    }

    pub(crate) async fn store_conversation(
        &mut self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<(), AppError> {
        let conversation = self
            .create_conversation_with_metadata(content, message_type, None)
            .await?;
        self.conversations.push(conversation.clone());

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation))
            .await;

        Ok(())
    }

    pub(crate) async fn create_conversation(
        &self,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Result<ConversationRecord, AppError> {
        self.create_conversation_with_metadata(content, message_type, None)
            .await
    }

    pub(crate) async fn create_conversation_with_metadata(
        &self,
        content: ConversationContent,
        message_type: ConversationMessageType,
        metadata: Option<Value>,
    ) -> Result<ConversationRecord, AppError> {
        self.create_conversation_with_id(
            self.ctx.app_state.sf.next_id()? as i64,
            content,
            message_type,
            metadata,
        )
        .await
    }

    pub(crate) async fn create_conversation_with_id(
        &self,
        conversation_id: i64,
        content: ConversationContent,
        message_type: ConversationMessageType,
        metadata: Option<Value>,
    ) -> Result<ConversationRecord, AppError> {
        let mut command = CreateConversationCommand::new(
            conversation_id,
            self.ctx.thread_id,
            content,
            message_type,
        )
        .with_execution_run_id(self.ctx.execution_run_id);
        if let Some(metadata) = metadata {
            command = command.with_metadata(metadata);
        }
        command
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
    }

    pub(crate) async fn store_user_message(
        &self,
        message: String,
        images: Option<Vec<dto::json::agent_executor::ImageData>>,
    ) -> Result<ConversationRecord, AppError> {
        let model_files = if let Some(imgs) = images {
            let mut uploaded_files = Vec::new();

            for img in imgs {
                use base64::{engine::general_purpose::STANDARD, Engine};
                let bytes = STANDARD.decode(&img.data).map_err(|e| {
                    AppError::BadRequest(format!("Invalid base64 image data: {}", e))
                })?;

                let file_extension = img.mime_type.split('/').last().unwrap_or("png");
                let filename = format!("{}.{}", self.ctx.app_state.sf.next_id()?, file_extension);

                let relative_path = self.filesystem.save_upload(&filename, &bytes).await?;

                uploaded_files.push(models::FileData {
                    filename,
                    mime_type: img.mime_type,
                    url: relative_path,
                    size_bytes: Some(bytes.len() as u64),
                });
            }

            Some(uploaded_files)
        } else {
            None
        };

        let command = CreateConversationCommand::new(
            self.ctx.app_state.sf.next_id()? as i64,
            self.ctx.thread_id,
            ConversationContent::UserMessage {
                message,
                sender_name: None,
                files: model_files,
            },
            ConversationMessageType::UserMessage,
        )
        .with_execution_run_id(self.ctx.execution_run_id);
        let conversation = command
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(conversation.clone()))
            .await;

        Ok(conversation)
    }

    pub(crate) async fn get_conversation_history_for_llm(&self) -> Vec<LlmHistoryEntry> {
        let mut history = Vec::new();
        let ordered_conversations = self
            .conversations
            .iter()
            .filter(|conv| matches!(conv.message_type, ConversationMessageType::ExecutionSummary))
            .chain(self.conversations.iter().filter(|conv| {
                !matches!(conv.message_type, ConversationMessageType::ExecutionSummary)
            }))
            .collect::<Vec<_>>();
        let mut i = 0;

        while i < ordered_conversations.len() {
            let conv = ordered_conversations[i];
            let timestamp = Some(conv.created_at.to_rfc3339());

            match conv.message_type {
                ConversationMessageType::ExecutionSummary => {
                    if let ConversationContent::ExecutionSummary {
                        user_message,
                        agent_execution,
                    } = &conv.content
                    {
                        history.push(LlmHistoryEntry::with_content(
                            "model",
                            "execution_summary",
                            timestamp.clone(),
                            format!(
                                "[Compressed prior thread history]\nThis entry was generated to condense older conversation history while preserving important decisions, constraints, failures, corrections, and results. Treat it as archival context for what already happened, not as a fresh user request.\nHistorical anchor: {}\n{}",
                                user_message,
                                agent_execution
                            ),
                        ));

                        i += 1;
                    }
                }
                ConversationMessageType::UserMessage => {
                    if let ConversationContent::UserMessage {
                        message,
                        files,
                        sender_name,
                    } = &conv.content
                    {
                        let mut parts = vec![LlmHistoryPart::text(message.clone())];

                        if let Some(file_list) = files {
                            for file in file_list {
                                let attachment_note = if file.mime_type.starts_with("image/") {
                                    format!(
                                        "[Attached image: {} ({}). Call read_image(path=\"{}\") to analyze it.]",
                                        file.filename, file.url, file.url
                                    )
                                } else {
                                    format!("\n[Attached: {} ({})]", file.filename, file.url)
                                };
                                parts.push(LlmHistoryPart::text(attachment_note));
                            }
                        }

                        let mut entry = LlmHistoryEntry::with_parts(
                            "user",
                            conversation_message_type_label(&conv.message_type),
                            timestamp.clone(),
                            parts,
                        );
                        entry.sender = sender_name.clone();
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    } else {
                        let mut entry = LlmHistoryEntry::with_content(
                            "user",
                            conversation_message_type_label(&conv.message_type),
                            timestamp.clone(),
                            self.extract_conversation_content(&conv.content),
                        );
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    }
                    i += 1;
                }
                ConversationMessageType::ToolResult => {
                    let content_value =
                        serde_json::to_value(&conv.content).unwrap_or_else(|_| json!({}));
                    let mut inline_parts: Vec<LlmHistoryPart> = Vec::new();

                    if matches!(conv.message_type, ConversationMessageType::ToolResult) {
                        let tool_name = content_value
                            .get("tool_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        if tool_name == "read_image" {
                            let path = content_value
                                .get("output")
                                .and_then(|v| v.get("data"))
                                .and_then(|v| v.get("path"))
                                .and_then(|v| v.as_str());
                            let mime_type = content_value
                                .get("output")
                                .and_then(|v| v.get("data"))
                                .and_then(|v| v.get("mime_type"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("application/octet-stream");

                            if let Some(path) = path {
                                if let Ok(bytes) = self.filesystem.read_file_bytes(path).await {
                                    use base64::{engine::general_purpose::STANDARD, Engine};
                                    let base64_data = STANDARD.encode(bytes);
                                    inline_parts
                                        .push(LlmHistoryPart::inline_data(mime_type, base64_data));
                                }
                            }
                        }
                    }

                    let serialized = serde_json::to_string(&content_value).unwrap_or_default();
                    let mut parts = vec![LlmHistoryPart::text(serialized)];
                    parts.extend(inline_parts);

                    let mut entry = LlmHistoryEntry::with_parts(
                        self.map_conversation_type_to_role(&conv.message_type),
                        conversation_message_type_label(&conv.message_type),
                        timestamp.clone(),
                        parts,
                    );
                    entry.metadata = conv.metadata.clone();
                    history.push(entry);
                    i += 1;
                }
                ConversationMessageType::SystemDecision => {
                    if let Some(entry) = self.system_decision_history_entry(conv) {
                        history.push(entry);
                    }
                    i += 1;
                }
                _ => {
                    let mut entry = LlmHistoryEntry::with_content(
                        self.map_conversation_type_to_role(&conv.message_type),
                        conversation_message_type_label(&conv.message_type),
                        timestamp,
                        self.extract_conversation_content(&conv.content),
                    );
                    entry.metadata = conv.metadata.clone();
                    history.push(entry);
                    i += 1;
                }
            }
        }

        history
    }

    fn system_decision_history_entry(&self, conv: &ConversationRecord) -> Option<LlmHistoryEntry> {
        let content_value = serde_json::to_value(&conv.content).ok()?;
        let mut parts = Vec::new();

        if let ConversationContent::SystemDecision {
            step, reasoning, ..
        } = &conv.content
        {
            let runtime_correction = matches!(
                step.as_str(),
                STRUCTURED_OUTPUT_TRUNCATED_STEP
                    | TOOL_LOAD_REQUIRED_STEP
                    | DATABASE_ERROR_RETRY_STEP
                    | LLM_REQUEST_FAILED_STEP
                    | RETRYABLE_EXECUTION_ERROR_STEP
            );

            if runtime_correction {
                parts.push(LlmHistoryPart::text(format!(
                    "[Runtime correction] {}",
                    reasoning
                )));
            }
        }

        parts.push(LlmHistoryPart::text(
            serde_json::to_string(&content_value).unwrap_or_default(),
        ));

        let mut entry = LlmHistoryEntry::with_parts(
            self.map_conversation_type_to_role(&conv.message_type),
            "system_decision",
            Some(conv.created_at.to_rfc3339()),
            parts,
        );
        entry.metadata = conv.metadata.clone();
        Some(entry)
    }

    pub(crate) fn map_conversation_type_to_role(
        &self,
        msg_type: &ConversationMessageType,
    ) -> &'static str {
        match msg_type {
            ConversationMessageType::UserMessage
            | ConversationMessageType::ApprovalResponse
            | ConversationMessageType::ToolResult => "user",
            _ => "model",
        }
    }

    pub(crate) fn extract_conversation_content(&self, content: &ConversationContent) -> String {
        match content {
            ConversationContent::UserMessage { message, .. } => message.clone(),
            ConversationContent::Steer { message, .. } => message.clone(),
            ConversationContent::ToolResult { .. } => {
                serde_json::to_string(content).unwrap_or_default()
            }
            ConversationContent::ApprovalRequest { description, .. } => description.clone(),
            ConversationContent::SystemDecision { .. } => String::new(),
            _ => serde_json::to_string(content).unwrap_or_default(),
        }
    }
}
use commands::UpdateAgentThreadStateCommand;

impl AgentExecutor {
    pub(crate) async fn compact_history_before_execution_if_needed(
        &mut self,
        trigger_conversation: &ConversationRecord,
    ) -> Result<bool, AppError> {
        const PROMPT_TOKEN_THRESHOLD: u32 = 120_000;

        if self
            .conversation_compaction_state
            .max_prompt_token_count_seen
            < PROMPT_TOKEN_THRESHOLD
        {
            return Ok(false);
        }

        let conversations = GetCompactionWindowConversationsQuery {
            thread_id: self.ctx.thread_id,
            before_conversation_id: trigger_conversation.id,
        }
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;

        let cleanup_through_id = conversations.iter().map(|conv| conv.id).max();
        let Some(cleanup_through_id) = cleanup_through_id else {
            return Ok(false);
        };

        let execution_messages: Vec<_> = conversations
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
            return Ok(false);
        }

        let summary_request = build_compaction_window_label(&conversations);
        let (_summary_tokens, summary_record) = self
            .generate_execution_summary_for_messages(summary_request, execution_messages)
            .await?;

        DispatchConversationCleanupTaskCommand::new(self.ctx.thread_id, cleanup_through_id)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).nats().id())
            .await?;

        let _ = self
            .channel
            .send(StreamEvent::PlatformEvent(
                "conversation_compacted".to_string(),
                json!({
                    "thread_id": self.ctx.thread_id.to_string(),
                    "summary_conversation_id": summary_record.id.to_string(),
                    "cleanup_through_id": cleanup_through_id.to_string(),
                    "trigger_conversation_id": trigger_conversation.id.to_string(),
                }),
            ))
            .await;

        self.conversation_compaction_state
            .max_prompt_token_count_seen = 0;
        self.conversation_compaction_state.last_prompt_token_count = 0;
        self.conversation_compaction_state.last_total_token_count = 0;
        self.conversation_compaction_state.last_compacted_at = Some(chrono::Utc::now());

        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        Ok(true)
    }

    async fn generate_execution_summary_for_messages(
        &mut self,
        user_request: String,
        execution_messages: Vec<serde_json::Value>,
    ) -> Result<(usize, ConversationRecord), AppError> {
        let template_context = json!({
            "user_request": user_request,
            "execution_messages": execution_messages,
        });
        let config: SemanticLlmPromptConfig =
            render_template_json(AgentTemplates::EXECUTION_SUMMARY, &template_context)?;
        let system_prompt = render_prompt_text("execution_summary_system", &template_context)?;
        let messages = execution_messages
            .iter()
            .filter_map(|message| {
                let role = message.get("role")?.as_str()?.to_string();
                let content = message.get("content")?.as_str()?.to_string();
                Some(SemanticLlmMessage::text(role, content))
            })
            .chain(std::iter::once(SemanticLlmMessage::text(
                "user",
                format!(
                    r#"Analyze this archival execution window.
Historical anchor: {}

Return a compact historical script map.
Preserve important user corrections, changed priorities, stop/continue instructions, exact failures, durable constraints, and verified file/path details.
Do not turn unfinished or speculative work into active instructions for the future.
Use OPEN only for a real blocker, required user input/approval/data, or genuinely incomplete work at the end of this compacted window.
Do not use OPEN for speculative next steps, stale unfinished ideas, or optional future improvements.
If later user turns superseded earlier goals, make that clear in the summary."#,
                    user_request
                ),
            )))
            .collect::<Vec<_>>();
        let request = SemanticLlmRequest::from_config(system_prompt, messages, config);

        let summary_response = self
            .create_weak_llm()
            .await?
            .generate_structured_from_prompt::<serde_json::Value>(request, None)
            .await
            .map(|output| output.value)
            .map_err(|e| AppError::Internal(format!("Summary generation failed: {e}")))?;

        let agent_execution = summary_response
            .get("agent_execution")
            .and_then(|v| v.as_str())
            .unwrap_or("Completed the requested task")
            .to_string();

        let summary_record = self
            .create_conversation(
                ConversationContent::ExecutionSummary {
                    user_message: user_request,
                    agent_execution,
                },
                ConversationMessageType::ExecutionSummary,
            )
            .await?;

        self.conversations.push(summary_record.clone());

        let _ = self
            .channel
            .send(StreamEvent::ConversationMessage(summary_record.clone()))
            .await;

        Ok((0, summary_record))
    }

    fn compact_execution_message(&self, message: &ConversationRecord) -> String {
        match &message.content {
            ConversationContent::UserMessage { message, .. } => {
                format!("USER {}", truncate_for_summary(message, 240))
            }
            ConversationContent::Steer { message, .. } => {
                format!("STEER {}", truncate_for_summary(message, 220))
            }
            ConversationContent::ApprovalRequest { description, tools } => format!(
                "APPROVAL_REQUEST description={} tools={}",
                truncate_for_summary(description, 180),
                tools
                    .iter()
                    .map(|tool| tool.tool_name.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            ConversationContent::ApprovalResponse { approvals, .. } => format!(
                "APPROVAL_RESPONSE approvals={}",
                approvals
                    .iter()
                    .map(|approval| format!("{}:{:?}", approval.tool_name, approval.mode))
                    .collect::<Vec<_>>()
                    .join(",")
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
            ConversationContent::ToolResult {
                tool_name,
                status,
                input,
                output,
                error,
            } => format!(
                "TOOL_RESULT tool={} status={} input={} preview={} error={}",
                tool_name,
                status,
                truncate_for_summary(&compact_json_preview(input, 180), 180),
                truncate_for_summary(
                    &output
                        .as_ref()
                        .map(|value| compact_json_preview(value, 180))
                        .unwrap_or_else(|| "no_output".to_string()),
                    180
                ),
                truncate_for_summary(error.as_deref().unwrap_or(""), 120)
            ),
            ConversationContent::ExecutionSummary {
                agent_execution, ..
            } => format!("SUMMARY {}", truncate_for_summary(agent_execution, 320)),
        }
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

fn build_compaction_window_label(conversations: &[ConversationRecord]) -> String {
    let prompts = conversations
        .iter()
        .filter_map(|conv| match &conv.content {
            ConversationContent::UserMessage { message, .. } => {
                Some(truncate_for_summary(message, 120))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if prompts.is_empty() {
        return "Compacted conversation history before current request".to_string();
    }

    let latest = prompts
        .last()
        .cloned()
        .unwrap_or_else(|| "Compacted conversation history before current request".to_string());
    let prior_turns = prompts.len().saturating_sub(1);

    if prior_turns > 0 {
        format!(
            "Latest prior user turn: {} | {} earlier user turns compacted",
            latest, prior_turns
        )
    } else {
        format!("Latest prior user turn: {}", latest)
    }
}

fn conversation_message_type_label(message_type: &ConversationMessageType) -> &'static str {
    match message_type {
        ConversationMessageType::UserMessage => "user_message",
        ConversationMessageType::Steer => "steer",
        ConversationMessageType::ToolResult => "tool_result",
        ConversationMessageType::SystemDecision => "system_decision",
        ConversationMessageType::ApprovalRequest => "approval_request",
        ConversationMessageType::ApprovalResponse => "approval_response",
        ConversationMessageType::ExecutionSummary => "execution_summary",
    }
}
