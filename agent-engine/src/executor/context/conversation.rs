use super::core::AgentExecutor;
use crate::executor::runtime::step_control::{
    DATABASE_ERROR_RETRY_STEP, LLM_REQUEST_FAILED_STEP, RETRYABLE_EXECUTION_ERROR_STEP,
};
use crate::llm::{SemanticLlmContentBlock, SemanticLlmMessage, SemanticLlmRequest};
use templatekit::render_prompt_text;

use commands::{CreateConversationCommand, DispatchConversationCleanupTaskCommand};
use common::error::AppError;
use dto::json::{LlmHistoryEntry, LlmHistoryPart, StreamEvent};
use models::{ConversationContent, ConversationMessageType, ConversationRecord};
use queries::GetCompactionWindowConversationsQuery;
use serde_json::{json, Value};

enum ClarificationOutcome<'a> {
    Answered(&'a [models::QuestionAnswer]),
    Expired,
    Pending,
}

pub(crate) fn format_relative_time_rfc3339(
    ts: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    match chrono::DateTime::parse_from_rfc3339(ts) {
        Ok(parsed) => {
            let delta_secs = (now - parsed.with_timezone(&chrono::Utc)).num_seconds();
            format_relative_delta(delta_secs)
        }
        Err(_) => ts.to_string(),
    }
}

fn format_relative_delta(delta_secs: i64) -> String {
    let abs = delta_secs.unsigned_abs();
    if abs < 5 {
        return "just now".to_string();
    }
    let is_future = delta_secs < 0;
    let (n, unit) = if abs < 60 {
        (abs, "s")
    } else if abs < 3600 {
        (abs / 60, "m")
    } else if abs < 86_400 {
        (abs / 3600, "h")
    } else {
        (abs / 86_400, "d")
    };
    if is_future {
        format!("in {n}{unit}")
    } else {
        format!("{n}{unit} ago")
    }
}

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
                    let relative = format_relative_time_rfc3339(timestamp, chrono::Utc::now());
                    return format!("[{relative}] {trimmed}");
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

    /// In-memory-only conversation entry used for runtime steering guards.
    /// Pushed onto `self.conversations` so the LLM history rendering picks it up
    /// for the remainder of this execution, but never written to Postgres or
    /// streamed to clients. Evaporates when the executor is dropped.
    pub(crate) fn store_transient_steer(&mut self, step: &str, reasoning: String) {
        let id = match self.ctx.app_state.sf.next_id() {
            Ok(v) => v as i64,
            Err(_) => 0,
        };
        let now = chrono::Utc::now();
        let conversation = ConversationRecord {
            id,
            thread_id: Some(self.ctx.thread_id),
            board_item_id: self.current_board_item_id(),
            execution_run_id: Some(self.ctx.execution_run_id),
            timestamp: now,
            content: ConversationContent::SystemDecision {
                step: step.to_string(),
                reasoning: reasoning.clone(),
                confidence: 1.0,
            },
            message_type: ConversationMessageType::SystemDecision,
            created_at: now,
            updated_at: now,
            metadata: None,
        };
        let is_guard_class = step.ends_with("_guard")
            || step.starts_with("complete_blocked")
            || step.starts_with("compaction_blocked");
        let board_item_id = self.current_board_item_id();
        if is_guard_class {
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                board_item_id = ?board_item_id,
                execution_run_id = self.ctx.execution_run_id,
                step = step,
                "guard fired"
            );
        } else {
            tracing::info!(
                thread_id = self.ctx.thread_id,
                board_item_id = ?board_item_id,
                execution_run_id = self.ctx.execution_run_id,
                step = step,
                "transient steer fired"
            );
        }
        self.conversations.push(conversation);
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

    pub(crate) async fn store_subscription_delivery_message(
        &self,
        summary: String,
    ) -> Result<ConversationRecord, AppError> {
        let mut command = CreateConversationCommand::new(
            self.ctx.app_state.sf.next_id()? as i64,
            self.ctx.thread_id,
            ConversationContent::TaskSubscriptionDelivery { summary },
            ConversationMessageType::TaskSubscriptionNotification,
        )
        .with_execution_run_id(self.ctx.execution_run_id);
        if let Some(board_item_id) = self.current_board_item_id() {
            command = command.with_board_item_id(board_item_id);
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
        let mut history: Vec<LlmHistoryEntry> = Vec::new();

        let ordered_conversations = self
            .conversations
            .iter()
            .filter(|conv| matches!(conv.message_type, ConversationMessageType::ExecutionSummary))
            .chain(self.conversations.iter().filter(|conv| {
                !matches!(conv.message_type, ConversationMessageType::ExecutionSummary)
            }))
            .collect::<Vec<_>>();

        let mut response_for_request: std::collections::HashMap<i64, &ConversationRecord> =
            std::collections::HashMap::new();
        let mut skip_conversation_ids: std::collections::HashSet<i64> =
            std::collections::HashSet::new();
        for conv in &ordered_conversations {
            if let ConversationContent::ClarificationResponse {
                request_message_id: Some(req_id),
                ..
            } = &conv.content
            {
                response_for_request.insert(*req_id, *conv);
            }
        }

        let mut i = 0;

        while i < ordered_conversations.len() {
            let conv = ordered_conversations[i];
            let timestamp = Some(conv.created_at.to_rfc3339());

            if skip_conversation_ids.contains(&conv.id) {
                i += 1;
                continue;
            }

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
                        message,
                        attachments,
                        ..
                    } = &conv.content
                    {
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
                        history.push(LlmHistoryEntry::with_content("model", "steer", None, text));
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

                        let narrative = Self::render_tool_event(
                            tool_name,
                            status.as_str(),
                            input,
                            output.as_ref(),
                            error.as_deref(),
                        );

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
                        let mut entry =
                            LlmHistoryEntry::with_content("model", "approval_request", None, text);
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

                ConversationMessageType::ClarificationRequest => {
                    if let ConversationContent::ClarificationRequest { questions, context } =
                        &conv.content
                    {
                        let parsed_questions: Vec<models::Question> =
                            serde_json::from_value(questions.clone()).unwrap_or_default();

                        let response = response_for_request.get(&conv.id);
                        let parsed_answers: Vec<models::QuestionAnswer> = response
                            .and_then(|resp| {
                                if let ConversationContent::ClarificationResponse {
                                    answers, ..
                                } = &resp.content
                                {
                                    serde_json::from_value(answers.clone()).ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or_default();

                        let outcome = if let Some(resp) = response {
                            skip_conversation_ids.insert(resp.id);
                            ClarificationOutcome::Answered(&parsed_answers)
                        } else if ordered_conversations[i + 1..]
                            .iter()
                            .any(|c| matches!(c.message_type, ConversationMessageType::UserMessage))
                        {
                            ClarificationOutcome::Expired
                        } else {
                            ClarificationOutcome::Pending
                        };

                        let text = Self::format_clarification_entry(
                            &parsed_questions,
                            context.as_deref(),
                            outcome,
                        );
                        let mut entry = LlmHistoryEntry::with_content(
                            "user",
                            "clarification",
                            timestamp.clone(),
                            text,
                        );
                        entry.metadata = conv.metadata.clone();
                        history.push(entry);
                    }
                    i += 1;
                }

                ConversationMessageType::ClarificationResponse => {
                    i += 1;
                }

                ConversationMessageType::TaskSubscriptionNotification => {
                    if let ConversationContent::TaskSubscriptionDelivery { summary } =
                        &conv.content
                    {
                        history.push(LlmHistoryEntry::with_content(
                            "user",
                            "task_subscription_delivery",
                            timestamp.clone(),
                            summary.clone(),
                        ));
                    }
                    i += 1;
                }
            }
        }

        history
    }

    pub(crate) async fn get_task_history_for_llm(&self) -> Vec<LlmHistoryEntry> {
        let own_thread_id = self.ctx.thread_id;

        let mut entries: Vec<(chrono::DateTime<chrono::Utc>, LlmHistoryEntry)> = Vec::new();
        let current_execution_run_id = self.ctx.execution_run_id;

        for conv in &self.conversations {
            let timestamp = Some(conv.created_at.to_rfc3339());
            let is_cross = matches!(conv.thread_id, Some(tid) if tid != own_thread_id);
            let cross_tid = conv.thread_id.filter(|tid| *tid != own_thread_id);
            let is_timeline = conv
                .execution_run_id
                .map(|id| id != current_execution_run_id)
                .unwrap_or(true);
            let tag = |body: String| -> String {
                match cross_tid {
                    Some(tid) => match self.task_thread_meta.iter().find(|m| m.thread_id == tid) {
                        Some(m) => {
                            format!(
                                "[thread #{tid} \"{}\" ({})] {body}",
                                m.title, m.thread_purpose
                            )
                        }
                        None => format!("[thread #{tid}] {body}"),
                    },
                    None => body,
                }
            };

            match conv.message_type {
                ConversationMessageType::ExecutionSummary => {
                    if let ConversationContent::ExecutionSummary {
                        user_message,
                        agent_execution,
                    } = &conv.content
                    {
                        let body = format!(
                            "[Compressed prior history]\nOriginal request: {user_message}\n\n{agent_execution}"
                        );
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                if is_cross { "user" } else { "model" },
                                "execution_summary",
                                timestamp,
                                tag(body),
                            ),
                        ));
                    }
                }
                ConversationMessageType::UserMessage => {
                    if let ConversationContent::UserMessage {
                        message,
                        sender_name,
                        ..
                    } = &conv.content
                    {
                        let mut entry = LlmHistoryEntry::with_content(
                            "user",
                            "user_message",
                            timestamp,
                            tag(message.clone()),
                        );
                        entry.sender = sender_name.clone();
                        entries.push((conv.created_at, entry));
                    }
                }
                ConversationMessageType::Steer => {
                    if let ConversationContent::Steer { message, .. } = &conv.content {
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                if is_cross { "user" } else { "model" },
                                "steer",
                                timestamp,
                                tag(message.trim().to_string()),
                            ),
                        ));
                    }
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
                        let narrative = if is_timeline {
                            let action = Self::describe_tool_action(tool_name, input);
                            format!(
                                "Tool call: {action}\n[output not preserved in timeline view — re-run this tool yourself if you need the content]"
                            )
                        } else {
                            Self::render_tool_event(
                                tool_name,
                                status.as_str(),
                                input,
                                output.as_ref(),
                                error.as_deref(),
                            )
                        };
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                "user",
                                "tool_result",
                                timestamp,
                                tag(narrative),
                            ),
                        ));
                    }
                }
                ConversationMessageType::SystemDecision => {
                    if let ConversationContent::SystemDecision { step, .. } = &conv.content {
                        if is_cross && step != "abort" {
                            continue;
                        }
                        if let Some(mut entry) = self.system_decision_history_entry(conv) {
                            if is_cross {
                                let body = entry.content.unwrap_or_default();
                                entry.content = Some(tag(body));
                                entry.role = "user".to_string();
                            }
                            entries.push((conv.created_at, entry));
                        }
                    }
                }
                ConversationMessageType::ApprovalRequest => {
                    if let ConversationContent::ApprovalRequest { description, tools } =
                        &conv.content
                    {
                        let mut text = String::from("Requested user approval to use tools:");
                        for t in tools {
                            text.push_str(&format!("\n  - {}", t.tool_name));
                        }
                        text.push_str(&format!("\nReason: {description}"));
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                if is_cross { "user" } else { "model" },
                                "approval_request",
                                timestamp,
                                tag(text),
                            ),
                        ));
                    }
                }
                ConversationMessageType::ApprovalResponse => {
                    if let ConversationContent::ApprovalResponse { approvals, .. } = &conv.content {
                        let mut text = String::from("User responded to approval request:");
                        for d in approvals {
                            let mode = match d.mode {
                                models::ToolApprovalMode::AllowOnce => "allowed once",
                                models::ToolApprovalMode::AllowAlways => "always allowed",
                            };
                            text.push_str(&format!("\n  - {}: {}", d.tool_name, mode));
                        }
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                "user",
                                "approval_response",
                                timestamp,
                                tag(text),
                            ),
                        ));
                    }
                }
                ConversationMessageType::ClarificationRequest => {
                    if let ConversationContent::ClarificationRequest { questions, context } =
                        &conv.content
                    {
                        let parsed: Vec<models::Question> =
                            serde_json::from_value(questions.clone()).unwrap_or_default();
                        let mut text = String::from("Asked the user:");
                        for q in &parsed {
                            text.push_str(&format!("\n- {}", q.text.trim()));
                        }
                        if let Some(ctx) = context.as_deref().filter(|s| !s.is_empty()) {
                            text.push_str(&format!("\nContext: {ctx}"));
                        }
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                if is_cross { "user" } else { "model" },
                                "clarification",
                                timestamp,
                                tag(text),
                            ),
                        ));
                    }
                }
                ConversationMessageType::TaskSubscriptionNotification => {
                    if let ConversationContent::TaskSubscriptionDelivery { summary } =
                        &conv.content
                    {
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                "user",
                                "task_subscription_delivery",
                                Some(conv.created_at.to_rfc3339()),
                                tag(summary.clone()),
                            ),
                        ));
                    }
                }
                ConversationMessageType::ClarificationResponse => {
                    if let ConversationContent::ClarificationResponse { answers, .. } =
                        &conv.content
                    {
                        let parsed: Vec<models::QuestionAnswer> =
                            serde_json::from_value(answers.clone()).unwrap_or_default();
                        let mut text = String::from("User answered:");
                        for a in &parsed {
                            text.push_str(&format!(
                                "\n- {}: {}",
                                a.question_id,
                                Self::describe_answer_value(&a.value)
                            ));
                        }
                        entries.push((
                            conv.created_at,
                            LlmHistoryEntry::with_content(
                                "user",
                                "clarification_response",
                                timestamp,
                                tag(text),
                            ),
                        ));
                    }
                }
            }
        }

        for event in &self.routing_events {
            let mut text = format!(
                "[Task event] task_routing reason={}",
                event.routing_reason.as_deref().unwrap_or("unspecified")
            );
            if let Some(coord) = event.coordinator_thread_id {
                text.push_str(&format!(" → coordinator #{coord}"));
            }
            if let Some(s) = event.summary.as_deref().filter(|s| !s.is_empty()) {
                text.push_str(&format!("\n  summary: {s}"));
            }
            if let Some(n) = event.note.as_deref().filter(|s| !s.is_empty()) {
                text.push_str(&format!("\n  note: {n}"));
            }
            entries.push((
                event.created_at,
                LlmHistoryEntry::with_content(
                    "user",
                    "task_event",
                    Some(event.created_at.to_rfc3339()),
                    text,
                ),
            ));
        }

        entries.sort_by_key(|(ts, _)| *ts);
        entries.into_iter().map(|(_, e)| e).collect()
    }

    fn format_clarification_entry(
        questions: &[models::Question],
        context: Option<&str>,
        outcome: ClarificationOutcome,
    ) -> String {
        let mut out = String::new();
        out.push_str("You asked me the following question(s):");
        for q in questions {
            out.push_str("\n- ");
            out.push_str(q.text.trim());
            let expected = Self::describe_answer_kind(&q.answer_kind);
            out.push_str(&format!("\n  (expected: {expected})"));
        }
        if let Some(ctx) = context.map(|s| s.trim()).filter(|s| !s.is_empty()) {
            out.push_str(&format!("\nContext you gave me: {ctx}"));
        }

        match outcome {
            ClarificationOutcome::Answered(answers) => {
                out.push_str("\n\nMy answers:");
                for q in questions {
                    let answer_text = answers
                        .iter()
                        .find(|a| a.question_id == q.id)
                        .map(|a| Self::describe_answer_value(&a.value))
                        .unwrap_or_else(|| "(no answer recorded)".to_string());
                    out.push_str(&format!("\n- {}: {}", q.text.trim(), answer_text));
                }
            }
            ClarificationOutcome::Expired => {
                out.push_str(
                    "\n\nI didn't answer that directly — I sent you a follow-up message instead, so treat this question as expired. Use my later message as my actual intent.",
                );
            }
            ClarificationOutcome::Pending => {
                out.push_str(
                    "\n\nI haven't answered yet. Wait for my response; don't ask again — one pending set at a time.",
                );
            }
        }

        out
    }

    fn describe_answer_kind(kind: &models::AnswerKind) -> String {
        match kind {
            models::AnswerKind::FreeText {
                placeholder,
                max_length,
            } => {
                let mut s = String::from("free text");
                if let Some(p) = placeholder.as_deref().filter(|s| !s.is_empty()) {
                    s.push_str(&format!(" — hint: {p}"));
                }
                if let Some(m) = max_length {
                    s.push_str(&format!(" (max {m} chars)"));
                }
                s
            }
            models::AnswerKind::SingleChoice {
                choices,
                allow_other,
            } => {
                let labels = choices
                    .iter()
                    .map(|c| c.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                if *allow_other {
                    format!("one of [{labels}] or a free-text 'other' value")
                } else {
                    format!("one of [{labels}]")
                }
            }
            models::AnswerKind::MultiChoice {
                choices,
                min_selected,
                max_selected,
            } => {
                let labels = choices
                    .iter()
                    .map(|c| c.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let bounds = match (min_selected, max_selected) {
                    (Some(min), Some(max)) => format!(", pick {min}-{max}"),
                    (Some(min), None) => format!(", at least {min}"),
                    (None, Some(max)) => format!(", at most {max}"),
                    (None, None) => String::new(),
                };
                format!("any of [{labels}]{bounds}")
            }
            models::AnswerKind::YesNo => String::from("yes / no"),
            models::AnswerKind::Number { min, max, unit } => {
                let mut s = String::from("a number");
                match (min, max) {
                    (Some(min), Some(max)) => s.push_str(&format!(" between {min} and {max}")),
                    (Some(min), None) => s.push_str(&format!(" ≥ {min}")),
                    (None, Some(max)) => s.push_str(&format!(" ≤ {max}")),
                    (None, None) => {}
                }
                if let Some(u) = unit.as_deref().filter(|s| !s.is_empty()) {
                    s.push_str(&format!(" {u}"));
                }
                s
            }
            models::AnswerKind::Date { min_date, max_date } => {
                let mut s = String::from("a date (yyyy-mm-dd)");
                match (min_date, max_date) {
                    (Some(a), Some(b)) => s.push_str(&format!(" between {a} and {b}")),
                    (Some(a), None) => s.push_str(&format!(" on or after {a}")),
                    (None, Some(b)) => s.push_str(&format!(" on or before {b}")),
                    (None, None) => {}
                }
                s
            }
            models::AnswerKind::Confirm {
                confirm_label,
                cancel_label,
            } => {
                format!("confirm ({confirm_label}) or cancel ({cancel_label})")
            }
        }
    }

    fn describe_answer_value(value: &models::AnswerValue) -> String {
        match value {
            models::AnswerValue::FreeText { value } => value.clone(),
            models::AnswerValue::SingleChoice { value } => value.clone(),
            models::AnswerValue::MultiChoice { values } => values.join(", "),
            models::AnswerValue::YesNo { value } => {
                if *value {
                    "yes".into()
                } else {
                    "no".into()
                }
            }
            models::AnswerValue::Number { value } => value.to_string(),
            models::AnswerValue::Date { value } => value.clone(),
            models::AnswerValue::Confirm { accepted } => {
                if *accepted {
                    "confirmed".into()
                } else {
                    "cancelled".into()
                }
            }
        }
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
        let model_originated = matches!(step.as_str(), "note" | "abort");

        let (role, text) = if model_originated {
            (
                "model",
                Self::format_agent_decision_narrative(step, reasoning),
            )
        } else {
            let body = if runtime_correction {
                reasoning.to_string()
            } else {
                Self::format_agent_decision_narrative(step, reasoning)
            };
            (
                "user",
                format!(
                    "[runtime — not from the user, not your prior output. harness directive. act this turn.]\n\
                     [{step}] {body}"
                ),
            )
        };

        let mut entry = LlmHistoryEntry::with_content(role, "system_decision", None, text);
        entry.metadata = conv.metadata.clone();
        Some(entry)
    }

    fn format_agent_decision_narrative(step: &str, reasoning: &str) -> String {
        match step {
            "note" => format!("{reasoning}"),
            "note_loop_guard" => format!("{reasoning}"),
            "tool_call_loop_guard" => format!("{reasoning}"),
            "empty_response_guard" => format!("{reasoning}"),
            "complete_blocked_by_task_graph" => {
                format!("{reasoning}")
            }
            _other => format!("{reasoning}"),
        }
    }

    /// Render one tool-result conversation row as natural prose for LLM history.
    /// Past actions read as a developer's terminal session — verb + arg + observation —
    /// not as `Tool X ran successfully\nInput: {...}\nOutput:` blocks.
    fn render_tool_event(
        tool_name: &str,
        status: &str,
        input: &Value,
        output: Option<&Value>,
        error: Option<&str>,
    ) -> String {
        let action = Self::describe_tool_action(tool_name, input);
        match status {
            "success" => {
                let body = output
                    .map(|v| Self::format_output_for_history(tool_name, v))
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && s != "(empty)");
                match body {
                    Some(body) => format!("You ran the tool: {action}\n\nIt produced:\n{body}"),
                    None => format!("You ran the tool: {action}\n\nIt produced no output."),
                }
            }
            "error" => {
                let detail = error.unwrap_or("(no detail)");
                format!(
                    "You ran the tool: {action}\n\nIt failed with: {detail}\n\nIf this result matters for the task, retry with different inputs or take a different approach. Do not pretend it succeeded or invent the output you wished it returned.",
                )
            }
            "pending" => {
                format!(
                    "You called the tool: {action}\n\nIt's waiting for my approval and hasn't run yet. Don't act as if it produced output until you see it execute.",
                )
            }
            other => {
                format!(
                    "You ran the tool: {action}\n\nIt returned status `{other}`. Treat the result as inconclusive — verify before relying on it.",
                )
            }
        }
    }

    /// Map (tool_name, input) → a natural-prose action phrase.
    /// Known internal tools get hand-tuned verb+arg forms.
    /// Unknown / custom / MCP tools fall back to `Called <name>(<compact-args>)`.
    fn describe_tool_action(tool_name: &str, input: &Value) -> String {
        let str_field =
            |key: &str| -> &str { input.get(key).and_then(|v| v.as_str()).unwrap_or_default() };
        let u64_field = |key: &str| -> Option<u64> { input.get(key).and_then(|v| v.as_u64()) };

        match tool_name {
            "read_file" => {
                let path = str_field("path");
                match (u64_field("start_line"), u64_field("end_line")) {
                    (Some(s), Some(e)) => format!("Read {path} lines {s}..{e}"),
                    (Some(s), None) => format!("Read {path} from line {s}"),
                    _ => format!("Read {path}"),
                }
            }
            "write_file" => {
                let path = str_field("path");
                if input
                    .get("append")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    format!("Appended to {path}")
                } else {
                    format!("Wrote {path}")
                }
            }
            "edit_file" => {
                let path = str_field("path");
                let s = u64_field("start_line").unwrap_or(0);
                let e = u64_field("end_line").unwrap_or(0);
                format!("Edited {path} lines {s}..{e}")
            }
            "execute_command" => {
                let cmd = Self::truncate_str(str_field("command"), 300);
                format!("Ran `{cmd}`")
            }
            "web_search" => {
                let obj = str_field("objective");
                if !obj.is_empty() {
                    format!("Searched web for {obj}")
                } else if let Some(qs) = input.get("search_queries").and_then(|v| v.as_array()) {
                    let joined = qs
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    if joined.is_empty() {
                        "Searched web".to_string()
                    } else {
                        format!("Searched web: {joined}")
                    }
                } else {
                    "Searched web".to_string()
                }
            }
            "url_content" => {
                let urls = input.get("urls").and_then(|v| v.as_array());
                match urls {
                    Some(arr) if !arr.is_empty() => {
                        let first = arr.first().and_then(|v| v.as_str()).unwrap_or("");
                        let rest = arr.len().saturating_sub(1);
                        if rest == 0 {
                            format!("Fetched {first}")
                        } else {
                            format!("Fetched {first} (+{rest} more)")
                        }
                    }
                    _ => "Fetched URL".to_string(),
                }
            }
            "search_knowledgebase" => {
                let q = str_field("query");
                if q.is_empty() {
                    "Searched knowledge base".to_string()
                } else {
                    format!("Searched knowledge base for \"{q}\"")
                }
            }
            "load_memory" => {
                let q = str_field("query");
                if q.is_empty() {
                    "Loaded memory".to_string()
                } else {
                    format!("Loaded memory for \"{q}\"")
                }
            }
            "save_memory" => {
                let category = str_field("category");
                let scope = str_field("scope");
                match (category.is_empty(), scope.is_empty()) {
                    (false, false) => format!("Saved memory ({category}, {scope})"),
                    (false, true) => format!("Saved memory ({category})"),
                    _ => "Saved memory".to_string(),
                }
            }
            "update_memory" => {
                let id = str_field("memory_id");
                if id.is_empty() {
                    "Updated memory".to_string()
                } else {
                    format!("Updated memory {id}")
                }
            }
            "read_image" => {
                let path = str_field("path");
                if path.is_empty() {
                    "Inspected image".to_string()
                } else {
                    format!("Inspected image {path}")
                }
            }
            "list_threads" => "Listed threads".to_string(),
            "create_thread" => {
                let title = str_field("title");
                if title.is_empty() {
                    "Created thread".to_string()
                } else {
                    format!("Created thread \"{title}\"")
                }
            }
            "update_thread" => {
                let id = str_field("thread_id");
                if id.is_empty() {
                    "Updated thread".to_string()
                } else {
                    format!("Updated thread {id}")
                }
            }
            "create_project_task" => {
                let title = str_field("title");
                if title.is_empty() {
                    "Created project task".to_string()
                } else {
                    format!("Created project task \"{title}\"")
                }
            }
            "update_project_task" => {
                let key = str_field("task_key");
                let status = str_field("status");
                match (key.is_empty(), status.is_empty()) {
                    (false, false) => format!("Updated project task {key} (status={status})"),
                    (false, true) => format!("Updated project task {key}"),
                    (true, false) => format!("Updated project task (status={status})"),
                    _ => "Updated project task".to_string(),
                }
            }
            "assign_project_task" => {
                let key = str_field("task_key");
                if key.is_empty() {
                    "Assigned project task".to_string()
                } else {
                    format!("Assigned project task {key}")
                }
            }
            "task_graph_add_node" => {
                let title = str_field("title");
                if title.is_empty() {
                    "Added task graph node".to_string()
                } else {
                    format!("Added task graph node \"{title}\"")
                }
            }
            "task_graph_add_dependency" => {
                let from = str_field("from_node_id");
                let to = str_field("to_node_id");
                format!("Added task graph dependency {from} → {to}")
            }
            "task_graph_mark_in_progress" => {
                format!(
                    "Marked task graph node {} in progress",
                    str_field("node_id")
                )
            }
            "task_graph_complete_node" => {
                format!("Completed task graph node {}", str_field("node_id"))
            }
            "task_graph_fail_node" => {
                format!("Failed task graph node {}", str_field("node_id"))
            }
            "task_graph_reset" => {
                let reason = str_field("reason");
                if reason.is_empty() {
                    "Reset task graph".to_string()
                } else {
                    format!("Reset task graph: {reason}")
                }
            }
            "search_tools" => {
                let queries = input.get("queries").and_then(|v| v.as_array());
                let apps = input.get("apps").and_then(|v| v.as_array());
                if let Some(arr) = queries.filter(|a| !a.is_empty()) {
                    let q = arr
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("Searched tools: {q}")
                } else if let Some(arr) = apps.filter(|a| !a.is_empty()) {
                    let names = arr
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("Browsed tools for apps: {names}")
                } else {
                    "Searched tools".to_string()
                }
            }
            "load_tools" => {
                let names = input.get("tool_names").and_then(|v| v.as_array());
                match names {
                    Some(arr) if !arr.is_empty() => {
                        let joined = arr
                            .iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("Loaded tools: {joined}")
                    }
                    _ => "Loaded tools".to_string(),
                }
            }
            "sleep" => {
                let ms = u64_field("duration_ms").unwrap_or(0);
                format!("Slept {ms}ms")
            }
            "abort_task" => {
                let directive = str_field("directive");
                if directive.is_empty() {
                    "Aborted task".to_string()
                } else {
                    format!("Aborted task ({directive})")
                }
            }
            "note" => "Noted".to_string(),
            _ => {
                let summary = Self::compact_input_summary(input);
                if summary.is_empty() {
                    format!("Called {tool_name}")
                } else {
                    format!("Called {tool_name}({summary})")
                }
            }
        }
    }

    /// Compact one-line summary of a tool input for unknown/custom tools.
    /// Renders as `key=val, key=val` for objects (first 3 fields), capped to ~80 chars.
    fn compact_input_summary(input: &Value) -> String {
        const MAX_TOTAL: usize = 200;
        const MAX_VAL: usize = 80;
        match input {
            Value::Object(map) => {
                let pairs: Vec<String> = map
                    .iter()
                    .take(3)
                    .map(|(k, v)| {
                        let val_str = match v {
                            Value::String(s) => format!("\"{}\"", Self::truncate_str(s, MAX_VAL)),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            Value::Null => "null".to_string(),
                            Value::Array(a) => format!("[{} items]", a.len()),
                            Value::Object(_) => "{...}".to_string(),
                        };
                        format!("{k}={val_str}")
                    })
                    .collect();
                let mut joined = pairs.join(", ");
                if map.len() > 3 {
                    joined.push_str(", ...");
                }
                Self::truncate_str(&joined, MAX_TOTAL).to_string()
            }
            Value::Null => String::new(),
            Value::String(s) => format!("\"{}\"", Self::truncate_str(s, MAX_TOTAL)),
            other => Self::truncate_str(&other.to_string(), MAX_TOTAL).to_string(),
        }
    }

    fn truncate_str(s: &str, max: usize) -> String {
        let count = s.chars().count();
        if count <= max {
            return s.to_string();
        }
        let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{truncated}…")
    }

    fn format_output_for_history(tool_name: &str, value: &Value) -> String {
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
                    preview.insert("content".to_string(), Value::String(numbered));
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

    pub(crate) fn map_conversation_type_to_role(
        &self,
        msg_type: &ConversationMessageType,
    ) -> &'static str {
        match msg_type {
            ConversationMessageType::UserMessage
            | ConversationMessageType::ApprovalResponse
            | ConversationMessageType::ClarificationResponse
            | ConversationMessageType::ToolResult => "user",
            _ => "model",
        }
    }
}
use commands::UpdateAgentThreadStateCommand;

impl AgentExecutor {
    #[tracing::instrument(
        name = "conversation.compact",
        skip(self, trigger_conversation),
        fields(
            thread_id = self.ctx.thread_id,
            board_item_id = ?self.current_board_item_id(),
            execution_run_id = self.ctx.execution_run_id,
            compacted = tracing::field::Empty,
        )
    )]
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

        let mut execution_messages: Vec<_> = conversations
            .iter()
            .filter_map(|msg| {
                let compact_content = self.compact_execution_message(msg);
                if compact_content.is_empty() {
                    return None;
                }

                Some((
                    msg.created_at,
                    json!({
                        "role": self.map_conversation_type_to_role(&msg.message_type),
                        "message_type": conversation_message_type_label(&msg.message_type),
                        "timestamp": msg.created_at.to_rfc3339(),
                        "thread_id": msg.thread_id.map(|t| t.to_string()).unwrap_or_default(),
                        "content": compact_content,
                    }),
                ))
            })
            .collect();

        if self.current_board_item_id().is_some() {
            for event in &self.routing_events {
                let mut content = format!(
                    "task_routing reason={}",
                    event.routing_reason.as_deref().unwrap_or("unspecified")
                );
                if let Some(coord) = event.coordinator_thread_id {
                    content.push_str(&format!(" → coordinator #{coord}"));
                }
                if let Some(s) = event.summary.as_deref().filter(|s| !s.is_empty()) {
                    content.push_str(&format!("\nsummary: {s}"));
                }
                if let Some(n) = event.note.as_deref().filter(|s| !s.is_empty()) {
                    content.push_str(&format!("\nnote: {n}"));
                }
                execution_messages.push((
                    event.created_at,
                    json!({
                        "role": "system",
                        "message_type": "task_event",
                        "timestamp": event.created_at.to_rfc3339(),
                        "content": content,
                    }),
                ));
            }
        }

        execution_messages.sort_by_key(|(ts, _)| *ts);
        let execution_messages: Vec<_> = execution_messages.into_iter().map(|(_, v)| v).collect();

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
                    r#"Compact this archival execution window into the Thought / Acted / Learnt / Open log described in the system prompt.
Historical anchor: {}

Preserve payload content from tool calls (email bodies, drafted text, file contents, fetched records, query results), user corrections verbatim, exact errors, and IDs.
Use OPEN only for genuine blockers or incomplete work at the end of the window.
Output plain text only — no JSON, no code fences, no surrounding prose."#,
                    user_request
                ),
            )))
            .collect::<Vec<_>>();
        let request = SemanticLlmRequest {
            system_prompt,
            messages,
            response_json_schema: serde_json::Value::Null,
            temperature: None,
            max_output_tokens: Some(4096),
            reasoning_effort: None,
            forced_tool_names: None,
        };

        let agent_execution = self
            .create_weak_llm()
            .await?
            .generate_text_from_prompt(request)
            .await
            .map(|output| output.text)
            .map_err(|e| AppError::Internal(format!("Summary generation failed: {e}")))?;

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
                format!("USER {}", message)
            }
            ConversationContent::Steer { message, .. } => {
                format!("STEER {}", message)
            }
            ConversationContent::ApprovalRequest { description, tools } => format!(
                "APPROVAL_REQUEST description={} tools={}",
                description,
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
                step, confidence, reasoning
            ),
            ConversationContent::ToolResult {
                tool_name,
                status,
                input,
                output,
                error,
            } => format!(
                "TOOL_RESULT tool={} status={}\n  input={}\n  output={}\n  error={}",
                tool_name,
                status,
                compact_json_preview(input, 4000),
                output
                    .as_ref()
                    .map(|value| compact_json_preview(value, 8000))
                    .unwrap_or_else(|| "no_output".to_string()),
                error.as_deref().unwrap_or("")
            ),
            ConversationContent::ExecutionSummary {
                agent_execution, ..
            } => format!("SUMMARY {}", agent_execution),
            ConversationContent::ClarificationRequest { questions, context } => format!(
                "CLARIFICATION_REQUEST questions={} context={}",
                compact_json_preview(questions, 4000),
                context.as_deref().unwrap_or("")
            ),
            ConversationContent::ClarificationResponse { answers, .. } => {
                format!(
                    "CLARIFICATION_RESPONSE answers={}",
                    compact_json_preview(answers, 4000)
                )
            }
            ConversationContent::TaskSubscriptionNotification {
                task_key,
                from_status,
                to_status,
                ..
            } => format!(
                "TASK_SUBSCRIPTION {} {}->{}",
                task_key, from_status, to_status
            ),
            ConversationContent::TaskSubscriptionDelivery { summary } => {
                format!("TASK_SUBSCRIPTION_DELIVERY {summary}")
            }
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
        ConversationMessageType::ClarificationRequest => "clarification_request",
        ConversationMessageType::ClarificationResponse => "clarification_response",
        ConversationMessageType::TaskSubscriptionNotification => "task_subscription_notification",
    }
}
