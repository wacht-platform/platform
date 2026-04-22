use super::core::AgentExecutor;
use crate::executor::runtime::step_control::{
    DATABASE_ERROR_RETRY_STEP, LLM_REQUEST_FAILED_STEP, RETRYABLE_EXECUTION_ERROR_STEP,
};
use crate::llm::{
    SemanticLlmContentBlock, SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest,
};
use templatekit::{AgentTemplates, render_prompt_text, render_template_json};

use commands::{CreateConversationCommand, DispatchConversationCleanupTaskCommand};
use common::error::AppError;
use dto::json::{LlmHistoryEntry, LlmHistoryPart, StreamEvent};
use models::{ConversationContent, ConversationMessageType, ConversationRecord};
use queries::GetCompactionWindowConversationsQuery;
use serde_json::{json, Value};

impl AgentExecutor {
    pub(crate) fn llm_history_entry_text(entry: &LlmHistoryEntry) -> String {
        let body = if !entry.parts.is_empty() {
            entry
                .parts
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

    pub(crate) fn semantic_message_from_history_entry(
        entry: &LlmHistoryEntry,
    ) -> SemanticLlmMessage {
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
        if let Some(board_item_id) = self.current_board_item_id() {
            command = command.with_board_item_id(board_item_id);
        }
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

        let mut command = CreateConversationCommand::new(
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
        if let Some(board_item_id) = self.current_board_item_id() {
            command = command.with_board_item_id(board_item_id);
        }
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
                                "[Compressed prior thread history — treat as archival context, not a fresh request]\nOriginal request: {}\n\n{}",
                                user_message, agent_execution
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
                                    format!("[Attached file: {} ({})]", file.filename, file.url)
                                };
                                parts.push(LlmHistoryPart::text(attachment_note));
                            }
                        }

                        let mut entry = LlmHistoryEntry::with_parts(
                            "user",
                            "user_message",
                            timestamp.clone(),
                            parts,
                        );
                        entry.sender = sender_name.clone();
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    }
                    i += 1;
                }

                ConversationMessageType::Steer => {
                    if let ConversationContent::Steer {
                        message, attachments, ..
                    } = &conv.content
                    {
                        // Render the message verbatim under role="model". Do NOT wrap it in a
                        // narrative like `I sent this message to the user: "..."` — the model
                        // imitates that structure in its own subsequent replies. Timestamps
                        // are also intentionally omitted from model turns; gap markers between
                        // entries provide any needed temporal context without making timestamps
                        // look like an output format the model should copy.
                        let mut text = message.trim().to_string();
                        if let Some(att_list) = attachments {
                            if !att_list.is_empty() {
                                let paths = att_list
                                    .iter()
                                    .map(|a| a.path.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                text.push_str(&format!("\n[attachments: {paths}]"));
                            }
                        }
                        history.push(LlmHistoryEntry::with_content(
                            "model",
                            "steer",
                            None,
                            text,
                        ));
                    }
                    i += 1;
                }

                ConversationMessageType::ToolResult => {
                    if let ConversationContent::ToolResult {
                        tool_name,
                        status,
                        input,
                        output,
                        error,
                    } = &conv.content
                    {
                        let mut inline_parts: Vec<LlmHistoryPart> = Vec::new();

                        // Special case: read_image embeds image bytes as inline data for vision.
                        if tool_name == "read_image" {
                            let path = output
                                .as_ref()
                                .and_then(|v| v.get("data"))
                                .and_then(|v| v.get("path"))
                                .and_then(|v| v.as_str());
                            let mime_type = output
                                .as_ref()
                                .and_then(|v| v.get("data"))
                                .and_then(|v| v.get("mime_type"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("application/octet-stream");
                            if let Some(path) = path {
                                if let Ok(bytes) = self.filesystem.read_file_bytes(path).await {
                                    use base64::{engine::general_purpose::STANDARD, Engine};
                                    inline_parts.push(LlmHistoryPart::inline_data(
                                        mime_type,
                                        STANDARD.encode(bytes),
                                    ));
                                }
                            }
                        }

                        let input_text = Self::format_input_for_history(input);
                        let narrative = match status.as_str() {
                            "success" => {
                                let output_text = output
                                    .as_ref()
                                    .map(|v| Self::format_output_for_history(tool_name, v))
                                    .unwrap_or_else(|| "(no output)".to_string());
                                format!(
                                    "Tool `{tool_name}` ran successfully.\nInput: {input_text}\nOutput:\n{output_text}"
                                )
                            }
                            "error" => {
                                let error_text =
                                    error.as_deref().unwrap_or("(no error detail provided)");
                                format!(
                                    "Tool `{tool_name}` failed.\nInput: {input_text}\nError: {error_text}"
                                )
                            }
                            "pending" => {
                                format!("Tool `{tool_name}` is pending (awaiting approval or async completion).\nInput: {input_text}")
                            }
                            other => {
                                format!("Tool `{tool_name}` returned status `{other}`.\nInput: {input_text}")
                            }
                        };

                        let mut parts = vec![LlmHistoryPart::text(narrative)];
                        parts.extend(inline_parts);

                        let mut entry = LlmHistoryEntry::with_parts(
                            "user",
                            "tool_result",
                            timestamp.clone(),
                            parts,
                        );
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    }
                    i += 1;
                }

                ConversationMessageType::SystemDecision => {
                    if let Some(entry) = self.system_decision_history_entry(conv) {
                        history.push(entry);
                    }
                    i += 1;
                }

                ConversationMessageType::ApprovalRequest => {
                    if let ConversationContent::ApprovalRequest { description, tools } =
                        &conv.content
                    {
                        let mut text =
                            format!("I requested user approval to use the following tools:");
                        for tool in tools {
                            if let Some(desc) = &tool.tool_description {
                                text.push_str(&format!("\n  - {} — {}", tool.tool_name, desc));
                            } else {
                                text.push_str(&format!("\n  - {}", tool.tool_name));
                            }
                        }
                        text.push_str(&format!("\nReason: {description}"));
                        text.push_str(
                            "\n[Waiting for the user to approve or deny before continuing.]",
                        );
                        // Model-role — omit timestamp prefix (see steer comment above).
                        let mut entry = LlmHistoryEntry::with_content(
                            "model",
                            "approval_request",
                            None,
                            text,
                        );
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    }
                    i += 1;
                }

                ConversationMessageType::ApprovalResponse => {
                    if let ConversationContent::ApprovalResponse { approvals, .. } = &conv.content {
                        let mut text = String::from("The user responded to my approval request:");
                        for decision in approvals {
                            let mode = match decision.mode {
                                models::ToolApprovalMode::AllowOnce => "allowed once",
                                models::ToolApprovalMode::AllowAlways => "always allowed",
                            };
                            text.push_str(&format!("\n  - {}: {}", decision.tool_name, mode));
                        }
                        let mut entry = LlmHistoryEntry::with_content(
                            "user",
                            "approval_response",
                            timestamp.clone(),
                            text,
                        );
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    }
                    i += 1;
                }

                ConversationMessageType::AssignmentEvent => {
                    if let ConversationContent::AssignmentEvent { summary, .. } = &conv.content {
                        let text = summary
                            .as_deref()
                            .unwrap_or("Assignment event (no summary)")
                            .to_string();
                        let mut entry = LlmHistoryEntry::with_content(
                            "user",
                            "assignment_event",
                            timestamp.clone(),
                            format!("[Task event]\n{text}"),
                        );
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    }
                    i += 1;
                }
            }
        }

        history
    }

    fn system_decision_history_entry(&self, conv: &ConversationRecord) -> Option<LlmHistoryEntry> {
        let ConversationContent::SystemDecision {
            step, reasoning, ..
        } = &conv.content
        else {
            return None;
        };

        let runtime_correction = matches!(
            step.as_str(),
            DATABASE_ERROR_RETRY_STEP | LLM_REQUEST_FAILED_STEP | RETRYABLE_EXECUTION_ERROR_STEP
        );

        let text = if runtime_correction {
            format!("[Runtime correction — {step}]\n{reasoning}")
        } else {
            Self::format_agent_decision_narrative(step, reasoning)
        };

        // Model-role entry — skip timestamp prefix so the model doesn't treat
        // `[2026-...T...] ` as part of its own output format.
        let mut entry = LlmHistoryEntry::with_content("model", "system_decision", None, text);
        entry.metadata = conv.metadata.clone();
        Some(entry)
    }

    fn format_agent_decision_narrative(step: &str, reasoning: &str) -> String {
        match step {
            "note" => format!("[Note]\n{reasoning}"),
            "note_loop_guard" => format!("[System — note guard]\n{reasoning}"),
            "tool_call_loop_guard" => format!("[System — tool-call loop guard]\n{reasoning}"),
            "empty_response_guard" => format!("[System — empty response]\n{reasoning}"),
            "complete_blocked_by_task_graph" => {
                format!("[System — task graph still has unfinished work]\n{reasoning}")
            }
            other => format!("[System — {other}]\n{reasoning}"),
        }
    }

    // Full output — no truncation. The decision LLM needs every byte to reason correctly.
    // Context overflow is handled by the conversation compaction path, not here.
    fn format_output_for_history(tool_name: &str, value: &Value) -> String {
        // read_file stores raw content in the DB; number lines only for the
        // LLM view so the conversation record stays clean.
        if tool_name == "read_file" {
            if let Some(obj) = value.as_object() {
                let content = obj.get("content").and_then(|v| v.as_str());
                let start_line = obj.get("start_line").and_then(|v| v.as_u64());
                if let (Some(content), Some(start)) = (content, start_line) {
                    let numbered = content
                        .lines()
                        .enumerate()
                        .map(|(i, line)| {
                            crate::filesystem::AgentFilesystem::format_numbered_line(
                                start as usize + i,
                                line,
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let mut preview = obj.clone();
                    preview.insert(
                        "content".to_string(),
                        Value::String(numbered),
                    );
                    return serde_json::to_string_pretty(&Value::Object(preview))
                        .unwrap_or_else(|_| value.to_string());
                }
            }
        }

        match value {
            Value::String(s) => s.clone(),
            Value::Null => "(empty)".to_string(),
            _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        }
    }

    // Input is just an echo of what was sent — cap it to avoid wasting context on verbose params.
    fn format_input_for_history(value: &Value) -> String {
        const MAX: usize = 800;
        let raw = match value {
            Value::String(s) => s.clone(),
            Value::Null => "(empty)".to_string(),
            _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        };
        let char_count = raw.chars().count();
        if char_count > MAX {
            let truncated: String = raw.chars().take(MAX).collect();
            format!("{}… [truncated — {} chars total]", truncated, char_count)
        } else {
            raw
        }
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

        if self.is_service_mode_execution() && !self.service_mode_journal_was_updated().await? {
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "compaction_blocked_by_journal_guard".to_string(),
                    reasoning: "Conversation compaction was blocked because /task/JOURNAL.md has not been updated since the last checkpoint. The journal is the lossy summary that survives compaction — write it before the window is dropped. Update /task/JOURNAL.md with the key facts from the recent turns, then continue; compaction will retry on the next trigger.".to_string(),
                    confidence: 1.0,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;
            return Ok(false);
        }

        let conversations = GetCompactionWindowConversationsQuery {
            thread_id: self.ctx.thread_id,
            before_conversation_id: trigger_conversation.id,
            board_item_id: self.current_board_item_id(),
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
            .with_board_item_id(self.current_board_item_id())
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

        if self.current_board_item_id().is_some() {
            self.task_journal_start_hash = Some(
                crate::runtime::task_workspace::compute_task_journal_hash(&self.filesystem).await?,
            );
        }

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
            ConversationContent::AssignmentEvent {
                kind,
                assignment_id,
                summary,
                ..
            } => format!(
                "ASSIGNMENT_EVENT kind={:?} assignment_id={} summary={}",
                kind,
                assignment_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "-".to_string()),
                truncate_for_summary(summary.as_deref().unwrap_or(""), 180)
            ),
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
        ConversationMessageType::AssignmentEvent => "assignment_event",
    }
}
