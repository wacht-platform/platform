use super::core::AgentExecutor;

use common::error::AppError;
use dto::json::{
    AgentLoopContext, AgentLoopConversationContext, AgentLoopPromptEnvelope,
    AgentLoopResourceContext, AgentLoopRuntimeContext, AgentLoopTaskContext,
    AgentLoopThreadContext, KnowledgeBasePromptItem, LlmHistoryEntry, LlmHistoryPart,
    ToolPromptItem,
};

use crate::llm::{SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest};
use queries::GetProjectTaskBoardItemAssignmentByIdQuery;
use templatekit::{render_prompt_text, render_template_only, AgentTemplates};
const STEER_VISIBILITY_NUDGE_WINDOW: usize = 4;
const MAX_LIVE_CONTEXT_CHARS: usize = 12_000;
const MAX_TASK_JOURNAL_TAIL_CHARS: usize = 4_000;
const MAX_AGENT_DESCRIPTION_CHARS: usize = 1_000;
const MAX_SIBLING_TAIL_MESSAGE_CHARS: usize = 1_000;

fn cap_sibling_message_body(body: String) -> String {
    if body.chars().count() <= MAX_SIBLING_TAIL_MESSAGE_CHARS {
        return body;
    }
    let mut out: String = body.chars().take(MAX_SIBLING_TAIL_MESSAGE_CHARS).collect();
    out.push_str("…[truncated]");
    out
}

fn render_sibling_message_kind(content: &models::ConversationContent) -> String {
    match content {
        models::ConversationContent::UserMessage { .. } => "user_message",
        models::ConversationContent::Steer { .. } => "steer",
        models::ConversationContent::ToolResult { .. } => "tool_result",
        models::ConversationContent::SystemDecision { .. } => "system_decision",
        models::ConversationContent::ApprovalRequest { .. } => "approval_request",
        models::ConversationContent::ApprovalResponse { .. } => "approval_response",
        models::ConversationContent::ExecutionSummary { .. } => "execution_summary",
        models::ConversationContent::ClarificationRequest { .. } => "clarification_request",
        models::ConversationContent::ClarificationResponse { .. } => "clarification_response",
        models::ConversationContent::TaskSubscriptionNotification { .. } => {
            "task_subscription_notification"
        }
        models::ConversationContent::TaskSubscriptionDelivery { .. } => {
            "task_subscription_delivery"
        }
        models::ConversationContent::AssignmentExecutionTrigger { .. } => {
            "assignment_execution_trigger"
        }
        models::ConversationContent::TaskRoutingTrigger { .. } => "task_routing_trigger",
    }
    .to_string()
}

fn render_sibling_message_body(content: &models::ConversationContent) -> String {
    match content {
        models::ConversationContent::UserMessage { message, .. } => message.clone(),
        models::ConversationContent::Steer { message, .. } => message.clone(),
        models::ConversationContent::SystemDecision { reasoning, .. } => reasoning.clone(),
        models::ConversationContent::ToolResult {
            tool_name,
            status,
            input,
            output,
            error,
        } => {
            let input_s = serde_json::to_string(input).unwrap_or_else(|_| "<unrenderable>".into());
            let output_s = match output {
                Some(v) => serde_json::to_string(v).unwrap_or_else(|_| "<unrenderable>".into()),
                None => "null".to_string(),
            };
            let err = error
                .as_deref()
                .map(|e| format!(" error={e}"))
                .unwrap_or_default();
            format!("{tool_name} status={status} input={input_s} output={output_s}{err}")
        }
        models::ConversationContent::ApprovalRequest { description, .. } => description.clone(),
        models::ConversationContent::ApprovalResponse { approvals, .. } => {
            serde_json::to_string(approvals).unwrap_or_else(|_| "<unrenderable>".into())
        }
        models::ConversationContent::ExecutionSummary {
            user_message,
            agent_execution,
        } => format!("request: {user_message}\n\n{agent_execution}"),
        models::ConversationContent::ClarificationRequest { questions, .. } => {
            serde_json::to_string(questions).unwrap_or_else(|_| "<unrenderable>".into())
        }
        models::ConversationContent::ClarificationResponse {
            freeform_text,
            answers,
            ..
        } => match freeform_text {
            Some(t) if !t.trim().is_empty() => t.clone(),
            _ => serde_json::to_string(answers).unwrap_or_else(|_| "<unrenderable>".into()),
        },
        models::ConversationContent::TaskSubscriptionNotification {
            task_key,
            task_title,
            from_status,
            to_status,
            ..
        } => format!("subscription: {task_key} \"{task_title}\" {from_status}→{to_status}"),
        models::ConversationContent::TaskSubscriptionDelivery { summary } => summary.clone(),
        models::ConversationContent::AssignmentExecutionTrigger {
            task_key,
            assignment_id,
            ..
        } => format!("assignment_execution task={task_key} assignment={assignment_id}"),
        models::ConversationContent::TaskRoutingTrigger {
            task_key,
            routing_reason,
            ..
        } => format!(
            "task_routing task={task_key} reason={}",
            routing_reason.as_deref().unwrap_or("unspecified")
        ),
    }
}

fn truncate_prompt_text(value: Option<String>, max_chars: usize) -> Option<String> {
    let value = value?.trim().to_string();
    if value.chars().count() <= max_chars {
        return Some(value);
    }
    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated = truncated.trim_end().to_string();
    truncated.push_str("...");
    Some(truncated)
}

pub(crate) struct ThreadModeContext {
    pub(crate) exec_context: models::AgentThreadState,
    pub(crate) is_coordinator_thread: bool,
    pub(crate) allows_user_interaction: bool,
}

struct ConversationPromptContext {
    conversation_history_prefix: Vec<LlmHistoryEntry>,
    current_request_entry: LlmHistoryEntry,
}

pub(crate) struct BoardPromptContext {
    pub(crate) active_assignment: Option<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) active_board_item: Option<dto::json::ProjectTaskBoardPromptItem>,
    pub(crate) active_board_item_assignments: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) recent_assignment_history: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) task_journal_tail: Option<String>,
    pub(crate) thread_assignment_queue: Vec<dto::json::ProjectTaskBoardAssignmentPromptItem>,
    pub(crate) scoped_project_task_board_items: Vec<dto::json::ProjectTaskBoardPromptItem>,
}

pub(crate) struct ToolPromptContext {
    pub(crate) tool_prompt_items: Vec<ToolPromptItem>,
    pub(crate) knowledge_base_prompt_items: Vec<KnowledgeBasePromptItem>,
    pub(crate) available_sub_agents: Vec<dto::json::SubAgentPromptInfo>,
    pub(crate) discoverable_external_tool_names: Vec<String>,
    pub(crate) loaded_external_tool_names: Vec<String>,
    pub(crate) connected_external_integrations: Vec<String>,
}

impl AgentExecutor {
    /// Scan in-memory conversations for the most recent user-originated
    /// event. Surfaced as the MOST RECENT USER INPUT block at the top
    /// of the live context so the agent always reads the freshest steer
    /// first, even when older trigger markers in history are thin stubs.
    fn latest_user_input_snapshot(
        &self,
    ) -> Option<dto::json::template_context::MostRecentUserInput> {
        let mut latest: Option<(chrono::DateTime<chrono::Utc>, &'static str, String)> = None;
        for conv in self.conversations.iter() {
            let candidate: Option<(&'static str, String)> = match &conv.content {
                models::ConversationContent::UserMessage { message, .. } => {
                    let trimmed = message.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(("user_message", trimmed.to_string()))
                    }
                }
                models::ConversationContent::Steer { message, .. } => {
                    let trimmed = message.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(("steer", trimmed.to_string()))
                    }
                }
                models::ConversationContent::ClarificationResponse { freeform_text, .. } => {
                    freeform_text
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|t| ("freeform_clarification", t.to_string()))
                }
                _ => None,
            };
            if let Some((source, text)) = candidate {
                let ts = conv.created_at;
                let take = match latest.as_ref() {
                    Some((existing, _, _)) => ts > *existing,
                    None => true,
                };
                if take {
                    latest = Some((ts, source, text));
                }
            }
        }
        latest.map(
            |(ts, source, text)| dto::json::template_context::MostRecentUserInput {
                source: source.to_string(),
                text,
                timestamp: ts.to_rfc3339(),
            },
        )
    }

    fn last_sibling_thread_tail(
        &self,
        limit: usize,
    ) -> Option<dto::json::template_context::LastSiblingThreadTail> {
        let own = self.ctx.thread_id;
        let latest_sibling_thread = self
            .conversations
            .iter()
            .filter_map(|c| match c.thread_id {
                Some(tid) if tid != own => Some((tid, c.created_at)),
                _ => None,
            })
            .max_by_key(|(_, ts)| *ts)
            .map(|(tid, _)| tid)?;

        let mut rows: Vec<_> = self
            .conversations
            .iter()
            .filter(|c| c.thread_id == Some(latest_sibling_thread))
            .collect();
        rows.sort_by_key(|c| c.created_at);
        let tail = rows.split_off(rows.len().saturating_sub(limit));

        let label = self
            .task_thread_meta
            .iter()
            .find(|m| m.thread_id == latest_sibling_thread)
            .map(|m| format!("\"{}\" ({})", m.title, m.thread_purpose))
            .unwrap_or_else(|| "(no metadata)".to_string());

        let messages = tail
            .into_iter()
            .map(|c| dto::json::template_context::SiblingTailMessage {
                timestamp: c.created_at.to_rfc3339(),
                kind: render_sibling_message_kind(&c.content),
                body: cap_sibling_message_body(render_sibling_message_body(&c.content)),
            })
            .collect();

        Some(dto::json::template_context::LastSiblingThreadTail {
            thread_id: latest_sibling_thread.to_string(),
            thread_label: label,
            messages,
        })
    }

    fn steer_visibility_nudge(&self, allows_user_interaction: bool) -> Option<String> {
        if !allows_user_interaction {
            return None;
        }

        let mut recent_visible_messages = Vec::new();
        for conv in self.conversations.iter().rev() {
            match conv.message_type {
                models::ConversationMessageType::UserMessage
                | models::ConversationMessageType::ClarificationResponse => {
                    if recent_visible_messages.is_empty() {
                        return None;
                    }
                    break;
                }
                models::ConversationMessageType::Steer
                | models::ConversationMessageType::ApprovalRequest
                | models::ConversationMessageType::ApprovalResponse
                | models::ConversationMessageType::ClarificationRequest
                | models::ConversationMessageType::ExecutionSummary => {
                    recent_visible_messages.push(conv);
                    if recent_visible_messages.len() >= STEER_VISIBILITY_NUDGE_WINDOW {
                        break;
                    }
                }
                _ => {}
            }
        }

        if recent_visible_messages.len() < STEER_VISIBILITY_NUDGE_WINDOW {
            return None;
        }

        if recent_visible_messages
            .iter()
            .any(|conv| matches!(conv.message_type, models::ConversationMessageType::Steer))
        {
            return None;
        }

        Some(
            "No visible message was sent to the user in the last 4 conversation-visible steps. Before continuing another non-trivial action run, emit a short progress text alongside your next tool call(s) to keep the user informed. Skip this only if the next step is a tiny immediate read/search/list action."
                .to_string(),
        )
    }

    pub(crate) async fn fetch_all_prompt_caches(
        &self,
    ) -> Result<
        (
            ThreadModeContext,
            models::ThreadTaskGraphSnapshot,
            BoardPromptContext,
            ToolPromptContext,
        ),
        AppError,
    > {
        let thread_mode = self.load_thread_mode_context().await?;
        let is_coordinator = thread_mode.is_coordinator_thread;

        let (task_graph, board_context, tool_context) = tokio::try_join!(
            self.fetch_task_graph_snapshot(),
            self.load_board_prompt_context(is_coordinator),
            self.load_tool_prompt_context(is_coordinator),
        )?;

        Ok((thread_mode, task_graph, board_context, tool_context))
    }

    pub(crate) async fn build_agent_loop_context_json(
        &mut self,
    ) -> Result<serde_json::Value, AppError> {
        let thread_mode = match self.thread_mode_cache.take() {
            Some(v) => v,
            None => self.load_thread_mode_context().await?,
        };
        let is_coordinator = thread_mode.is_coordinator_thread;

        let task_graph = self.ensure_task_graph_snapshot().await?;
        let task_graph_view = Self::render_task_graph_view(&task_graph);

        let (board_context, conversation_context, tool_context) = match (
            self.board_context_cache.take(),
            self.tool_context_cache.take(),
        ) {
            (Some(board), Some(tool)) => {
                let conv = self.build_conversation_prompt_context().await;
                (board, conv, tool)
            }
            _ => tokio::try_join!(
                self.load_board_prompt_context(is_coordinator),
                async { Ok::<_, AppError>(self.build_conversation_prompt_context().await) },
                self.load_tool_prompt_context(is_coordinator),
            )?,
        };
        let context = AgentLoopContext {
            runtime: AgentLoopRuntimeContext {
                current_datetime_utc: chrono::Utc::now()
                    .format("%Y-%m-%d %H:%M:%S UTC")
                    .to_string(),
                iteration_info: dto::json::IterationInfo {
                    current_iteration: self.current_iteration.max(1),
                    max_iterations: 50,
                },
                steer_visibility_nudge: self
                    .steer_visibility_nudge(thread_mode.allows_user_interaction),
            },
            conversation: AgentLoopConversationContext {
                user_request: self.user_request.clone(),
                triggering_event: self
                    .active_thread_event
                    .as_ref()
                    .map(Self::thread_event_prompt_item),
                input_safety_signals: self.derive_input_safety_signals(),
            },
            thread: AgentLoopThreadContext {
                id: self.ctx.thread_id,
                title: thread_mode.exec_context.title.clone(),
                purpose: if thread_mode.is_coordinator_thread {
                    models::agent_thread::purpose::COORDINATOR.to_string()
                } else {
                    thread_mode.exec_context.thread_purpose.clone()
                },
                responsibility: thread_mode.exec_context.responsibility.clone(),
            },
            resources: AgentLoopResourceContext {
                available_tools: tool_context.tool_prompt_items,
                available_knowledge_bases: tool_context.knowledge_base_prompt_items,
                available_system_skills: Vec::new(),
                available_agent_skills: Vec::new(),
                available_sub_agents: tool_context.available_sub_agents,
            },
            task: AgentLoopTaskContext {
                project_task_board_items: board_context.scoped_project_task_board_items,
                active_board_item: board_context.active_board_item,
                active_assignment: board_context.active_assignment,
                active_board_item_assignments: board_context.active_board_item_assignments,
                recent_assignment_history: board_context.recent_assignment_history,
                task_journal_tail: board_context.task_journal_tail,
                thread_assignment_queue: board_context.thread_assignment_queue,
                task_graph_view,
            },
        };

        let mut prompt_context = AgentLoopPromptEnvelope {
            base: context,
            agent_name: self.ctx.agent.name.clone(),
            // The agent's own description is the agent's primary durable
            // instructions slot. Render in full — truncation only makes sense
            // for the sub-agent routing list (see `load_sub_agent_prompt_info`).
            agent_description: self
                .ctx
                .agent
                .description
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string),
            conversation_history_prefix: conversation_context.conversation_history_prefix.clone(),
            current_request_entry: conversation_context.current_request_entry,
            discoverable_external_tool_names: tool_context.discoverable_external_tool_names,
            loaded_external_tool_names: tool_context.loaded_external_tool_names,
            connected_external_integrations: tool_context.connected_external_integrations,
            custom_system_instructions: self
                .system_instructions
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
            most_recent_user_input: self.latest_user_input_snapshot(),
            last_sibling_thread_tail: self.last_sibling_thread_tail(5),
            live_context_message: None,
        };

        let mut prompt_context_value = serde_json::to_value(&prompt_context)?;
        Self::annotate_live_task_context_flag(&mut prompt_context_value);

        let live_context_message = render_template_only(
            AgentTemplates::AGENT_LOOP_LIVE_CONTEXT,
            &prompt_context_value,
        )
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to render agent loop live context template: {e}"
            ))
        })?;
        let live_context_message =
            truncate_prompt_text(Some(live_context_message), MAX_LIVE_CONTEXT_CHARS)
                .unwrap_or_default();

        prompt_context.live_context_message = Some(live_context_message);

        Ok(serde_json::to_value(&prompt_context)?)
    }

    pub(crate) fn build_agent_loop_messages(
        &self,
        conversation_history_prefix: &[LlmHistoryEntry],
        stable_context_message: Option<&str>,
        virtual_task_state_message: Option<&str>,
        live_context_message: &str,
        current_request_entry: &LlmHistoryEntry,
        trailing_user_message: Option<&str>,
    ) -> Vec<SemanticLlmMessage> {
        let mut messages = Vec::new();

        if let Some(message) = stable_context_message
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            messages.push(SemanticLlmMessage::text("user", message));
        }

        messages.extend(
            conversation_history_prefix
                .iter()
                .map(Self::semantic_message_from_history_entry),
        );

        if let Some(message) = virtual_task_state_message
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            messages.push(SemanticLlmMessage::text("user", message));
        }

        messages.push(SemanticLlmMessage::text("user", live_context_message));
        messages.push(Self::semantic_message_from_history_entry(
            current_request_entry,
        ));

        if let Some(message) = trailing_user_message
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            messages.push(SemanticLlmMessage::text("user", message));
        }

        messages
    }

    pub(crate) fn build_agent_loop_request(
        &self,
        prompt_context: &AgentLoopPromptEnvelope,
        prompt_context_value: &serde_json::Value,
        trailing_user_message: Option<&str>,
    ) -> Result<SemanticLlmRequest, AppError> {
        let live_context_message =
            prompt_context
                .live_context_message
                .as_deref()
                .ok_or_else(|| {
                    AppError::Internal(
                        "Next-step decision live context message missing".to_string(),
                    )
                })?;
        let reasoning_effort = if self.repeated_tool_call_count >= 2 {
            "medium"
        } else {
            "low"
        };
        let config = SemanticLlmPromptConfig {
            response_json_schema: serde_json::json!({}),
            temperature: None,
            max_output_tokens: Some(4096),
            reasoning_effort: Some(reasoning_effort.to_string()),
        };
        let system_prompt = render_prompt_text(self.system_prompt_name(), prompt_context_value)?;
        let stable_context_message = Self::build_stable_context_message(prompt_context);
        let virtual_task_state_message = Self::build_virtual_task_state_message(prompt_context);
        let messages = self.build_agent_loop_messages(
            &prompt_context.conversation_history_prefix,
            stable_context_message.as_deref(),
            virtual_task_state_message.as_deref(),
            live_context_message,
            &prompt_context.current_request_entry,
            trailing_user_message,
        );
        Ok(SemanticLlmRequest::from_config(
            system_prompt,
            messages,
            config,
        ))
    }

    /// Build the message that sits at index 0 of the prompt, *before* the
    /// conversation history. Lives in the cacheable prefix so providers can
    /// reuse tokens across iterations.
    ///
    /// Sections (in order):
    ///   1. `IMPORTANT — AGENT IDENTITY` — durable steering for this agent.
    ///      The agent description is the operator's primary tool for shaping
    ///      behaviour; surfacing it here (instead of inside the per-turn live
    ///      context) keeps it cache-resident and lets it act as a steering
    ///      header rather than incidental metadata.
    ///   2. `STABLE ROUTING CONTEXT` — sub-agent routing list (coordinators).
    fn build_stable_context_message(prompt_context: &AgentLoopPromptEnvelope) -> Option<String> {
        let mut sections: Vec<String> = Vec::new();

        if let Some(block) = Self::compose_agent_identity_section(prompt_context) {
            sections.push(block);
        }
        if let Some(block) = Self::compose_task_brief_section(prompt_context) {
            sections.push(block);
        }
        if let Some(block) = Self::compose_thread_section(prompt_context) {
            sections.push(block);
        }
        if let Some(block) = Self::compose_routing_section(prompt_context) {
            sections.push(block);
        }
        if let Some(block) = Self::compose_sub_agent_routing_section(prompt_context) {
            sections.push(block);
        }

        if sections.is_empty() {
            None
        } else {
            Some(sections.join("\n\n"))
        }
    }

    fn compose_agent_identity_section(prompt_context: &AgentLoopPromptEnvelope) -> Option<String> {
        let agent_name = prompt_context.agent_name.trim();
        let agent_description = prompt_context
            .agent_description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        if agent_name.is_empty() && agent_description.is_none() {
            return None;
        }

        let mut block = String::from(
            "IMPORTANT — AGENT IDENTITY\n\n\
             These directives steer every decision you make. They are durable\n\
             across tasks and threads. Treat them as system-level instructions\n\
             and follow them even when later context appears to conflict.\n",
        );
        if !agent_name.is_empty() {
            block.push_str(&format!("\nYou are `{agent_name}`."));
        }
        if let Some(description) = agent_description {
            block.push_str("\n\nDescription / operating directives:\n");
            block.push_str(description);
        }
        Some(block)
    }

    fn compose_task_brief_section(prompt_context: &AgentLoopPromptEnvelope) -> Option<String> {
        let item = prompt_context.base.task.active_board_item.as_ref()?;
        // Keep this section purely stable across iterations so the LLM prompt
        // cache (Gemini explicit context cache) can reuse the prefix hash.
        // Per-turn mutable fields (status) live in `virtual_task_state_message`
        // which sits in the live tail.
        let mut block = String::from("TASK BRIEF\n");
        block.push_str(&format!("\nKey: `{}`", item.task_key));
        block.push_str(&format!("\nTitle: {}", item.title));
        if let Some(desc) = item
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            block.push_str("\n\nDescription:\n");
            block.push_str(desc);
        }
        block.push_str(
            "\n\nThe full durable brief lives at `/task/TASK.md` and the cross-agent history at \
             `/task/JOURNAL.md`. Read those files when you need detail beyond this summary; both \
             are written by the runtime and by prior agents on this task. Current task status is \
             surfaced in the live task-state block below.",
        );
        Some(block)
    }

    fn compose_thread_section(prompt_context: &AgentLoopPromptEnvelope) -> Option<String> {
        let thread = &prompt_context.base.thread;
        let responsibility = thread
            .responsibility
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let custom = prompt_context
            .custom_system_instructions
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());

        let mut block = String::from("THREAD\n");
        block.push_str(&format!("\nID: #{}", thread.id));
        block.push_str(&format!("\nTitle: {}", thread.title));
        block.push_str(&format!("\nPurpose: {}", thread.purpose));
        if let Some(r) = responsibility {
            block.push_str("\nResponsibility: ");
            block.push_str(r);
        }
        if let Some(c) = custom {
            block.push_str("\n\nCustom operating instructions for this thread:\n");
            block.push_str(c);
        }
        Some(block)
    }

    fn compose_routing_section(prompt_context: &AgentLoopPromptEnvelope) -> Option<String> {
        let assignment = prompt_context.base.task.active_assignment.as_ref()?;
        // Same caching contract as `compose_task_brief_section`: keep stable
        // identifiers / role / note / instructions here, leave `status`
        // (which flips queued → running → succeeded/failed) to the live tail
        // via `virtual_task_state_message`.
        let mut block = String::from("ROUTING & ACTIVE ASSIGNMENT\n");
        block.push_str(&format!("\nAssignment ID: {}", assignment.assignment_id));
        block.push_str(&format!("\nBoard item: {}", assignment.board_item_id));
        block.push_str(&format!("\nRole: {}", assignment.assignment_role));
        if let Some(note) = assignment
            .note
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            block.push_str("\nRouting note: ");
            block.push_str(note);
        }
        if let Some(instructions) = assignment
            .instructions
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            block.push_str("\n\nInstructions for this assignment:\n");
            block.push_str(instructions);
        }
        Some(block)
    }

    fn build_virtual_task_state_message(
        prompt_context: &AgentLoopPromptEnvelope,
    ) -> Option<String> {
        let base = &prompt_context.base;
        let mut lines: Vec<String> = Vec::new();

        lines.push("CURRENT TASK STATE".to_string());

        let thread = &base.thread;
        lines.push(format!(
            "Thread: #{} \"{}\" purpose={}{}",
            thread.id,
            thread.title,
            thread.purpose,
            thread
                .responsibility
                .as_deref()
                .map(|r| format!(" responsibility={}", r))
                .unwrap_or_default()
        ));

        if let Some(item) = base.task.active_board_item.as_ref() {
            lines.push(format!(
                "Board item: {} — {} (status={})",
                item.task_key, item.title, item.status
            ));
            if !item.mounts.is_empty() {
                lines.push("Mounts:".to_string());
                for m in &item.mounts {
                    let desc = m
                        .description
                        .as_deref()
                        .filter(|s| !s.is_empty())
                        .map(|s| format!(" — {}", s))
                        .unwrap_or_default();
                    lines.push(format!("- {} ({}){}", m.mount_path, m.mode, desc));
                }
            }
        }

        if let Some(assignment) = base.task.active_assignment.as_ref() {
            lines.push(format!(
                "Active assignment: #{} role={} status={} thread={}",
                assignment.assignment_id,
                assignment.assignment_role,
                assignment.status,
                assignment.thread_id
            ));
        }

        if !base.task.active_board_item_assignments.is_empty() {
            lines.push("Board assignments:".to_string());
            for a in &base.task.active_board_item_assignments {
                lines.push(format!(
                    "- #{} role={} thread={} status={}",
                    a.assignment_id, a.assignment_role, a.thread_id, a.status
                ));
            }
        }

        if !base.task.recent_assignment_history.is_empty() {
            lines.push("Recent assignments:".to_string());
            for a in &base.task.recent_assignment_history {
                lines.push(format!(
                    "- #{} role={} thread={} status={}{}",
                    a.assignment_id,
                    a.assignment_role,
                    a.thread_id,
                    a.status,
                    a.result_status
                        .as_deref()
                        .map(|s| format!(" ({})", s))
                        .unwrap_or_default()
                ));
            }
        }

        if !base.task.thread_assignment_queue.is_empty() {
            lines.push("Queue:".to_string());
            for a in &base.task.thread_assignment_queue {
                lines.push(format!(
                    "- #{} role={} status={}",
                    a.assignment_id, a.assignment_role, a.status
                ));
            }
        }

        if let Some(tail) = base
            .task
            .task_journal_tail
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            lines.push("Journal tail:".to_string());
            lines.push(format!("```markdown\n{}\n```", tail));
        }

        if let Some(trigger) = base.conversation.triggering_event.as_ref() {
            lines.push(format!(
                "Trigger: `{}`",
                serde_json::to_string(trigger).unwrap_or_default()
            ));
        }

        if lines.len() <= 2 {
            None
        } else {
            Some(lines.join("\n"))
        }
    }

    fn compose_sub_agent_routing_section(
        prompt_context: &AgentLoopPromptEnvelope,
    ) -> Option<String> {
        let sub_agents = &prompt_context.base.resources.available_sub_agents;
        let mut routing_lines: Vec<String> = Vec::new();
        for sub_agent in sub_agents {
            let name = sub_agent.name.trim();
            if name.is_empty() {
                continue;
            }
            match sub_agent.description.as_deref().map(str::trim) {
                Some(description) if !description.is_empty() => {
                    routing_lines.push(format!("- {name}: {description}"));
                }
                _ => routing_lines.push(format!("- {name}")),
            }
        }
        if routing_lines.is_empty() {
            return None;
        }
        let mut block = String::from(
            "STABLE ROUTING CONTEXT\n\n\
             Assignable sub-agents. Use the exact `name` value as\n\
             `assigned_agent_name` when creating or routing lanes.\n",
        );
        for line in routing_lines {
            block.push('\n');
            block.push_str(&line);
        }
        Some(block)
    }

    fn annotate_live_task_context_flag(value: &mut serde_json::Value) {
        let triggering_event_present = value
            .get("conversation")
            .and_then(|v| v.get("triggering_event"))
            .map(|v| !v.is_null())
            .unwrap_or(false);

        let task = match value.get_mut("task").and_then(|v| v.as_object_mut()) {
            Some(t) => t,
            None => return,
        };

        let has_field = |key: &str| match task.get(key) {
            None | Some(serde_json::Value::Null) => false,
            Some(serde_json::Value::Array(a)) => !a.is_empty(),
            Some(serde_json::Value::String(s)) => !s.is_empty(),
            Some(serde_json::Value::Object(o)) => !o.is_empty(),
            Some(_) => true,
        };

        let has_any = triggering_event_present
            || has_field("active_assignment")
            || has_field("active_board_item")
            || has_field("active_board_item_assignments")
            || has_field("recent_assignment_history")
            || has_field("task_journal_tail")
            || has_field("thread_assignment_queue");

        task.insert(
            "has_live_context".to_string(),
            serde_json::Value::Bool(has_any),
        );
    }

    async fn load_thread_mode_context(&self) -> Result<ThreadModeContext, AppError> {
        let exec_context = self.ctx.get_thread().await?;
        let is_conversation_thread =
            exec_context.thread_purpose == models::agent_thread::purpose::CONVERSATION;
        let is_coordinator_thread = exec_context.thread_purpose
            == models::agent_thread::purpose::COORDINATOR
            || self
                .active_thread_event
                .as_ref()
                .map(|event| Self::thread_event_implies_coordinator(&event.event_type))
                .unwrap_or(false);

        Ok(ThreadModeContext {
            exec_context,
            is_coordinator_thread,
            allows_user_interaction: is_conversation_thread,
        })
    }

    async fn build_conversation_prompt_context(&self) -> ConversationPromptContext {
        let mut conversation_history = if self.current_board_item_id().is_some() {
            self.get_task_history_for_llm().await
        } else {
            self.get_conversation_history_for_llm().await
        };
        let current_request_entry = conversation_history.pop().unwrap_or_else(|| {
            LlmHistoryEntry::with_parts(
                "user",
                "user_message",
                None,
                vec![LlmHistoryPart::text(if self.user_request.trim().is_empty() {
                    "[No explicit current request message. Use the live context snapshot and recent history.]"
                } else {
                    self.user_request.as_str()
                })],
            )
        });

        ConversationPromptContext {
            conversation_history_prefix: conversation_history,
            current_request_entry,
        }
    }

    async fn load_board_prompt_context(
        &self,
        is_coordinator_thread: bool,
    ) -> Result<BoardPromptContext, AppError> {
        let active_assignment = self.load_active_assignment_prompt_item().await?;
        let active_board_item_id = active_assignment
            .as_ref()
            .map(|assignment| assignment.board_item_id)
            .or_else(|| {
                self.active_thread_event
                    .as_ref()
                    .and_then(|event| event.board_item_id)
            });
        let active_board_item = self
            .load_active_board_item_prompt_item(active_board_item_id)
            .await?;
        let active_board_item_assignments = self
            .load_active_board_item_assignments(active_board_item_id)
            .await;
        let recent_assignment_history =
            Self::recent_assignment_history(&active_board_item_assignments);
        let task_journal_tail = if active_board_item.is_some() {
            truncate_prompt_text(
                self.task_journal_tail_snippet().await?,
                MAX_TASK_JOURNAL_TAIL_CHARS,
            )
        } else {
            None
        };

        Ok(BoardPromptContext {
            thread_assignment_queue: self.load_thread_assignment_queue().await,
            scoped_project_task_board_items: self
                .scoped_project_task_board_items(is_coordinator_thread, active_board_item.as_ref()),
            active_assignment,
            active_board_item,
            active_board_item_assignments,
            recent_assignment_history,
            task_journal_tail,
        })
    }

    async fn load_tool_prompt_context(
        &self,
        is_coordinator_thread: bool,
    ) -> Result<ToolPromptContext, AppError> {
        let available_tools = self.available_tools_for_mode().await;

        let discoverable_external_tool_names = self
            .ctx
            .agent
            .tools
            .iter()
            .filter(|tool| !matches!(tool.tool_type, models::AiToolType::Internal))
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();
        let loaded_external_tool_names = self
            .loaded_external_tool_ids
            .iter()
            .filter_map(|tool_id| {
                self.ctx
                    .agent
                    .tools
                    .iter()
                    .find(|tool| tool.id == *tool_id)
                    .map(|tool| tool.name.clone())
            })
            .collect::<Vec<_>>();
        let tool_prompt_items = available_tools
            .iter()
            .map(ToolPromptItem::from_tool)
            .collect::<Vec<_>>();
        let knowledge_base_prompt_items = self
            .ctx
            .agent
            .knowledge_bases
            .iter()
            .map(KnowledgeBasePromptItem::from_knowledge_base)
            .collect::<Vec<_>>();

        let connected_external_integrations = self.load_connected_external_integrations().await;

        let available_sub_agents = if is_coordinator_thread {
            self.load_available_sub_agents().await
        } else {
            Vec::new()
        };

        Ok(ToolPromptContext {
            available_sub_agents,
            tool_prompt_items,
            knowledge_base_prompt_items,
            discoverable_external_tool_names,
            loaded_external_tool_names,
            connected_external_integrations,
        })
    }

    async fn load_connected_external_integrations(&self) -> Vec<String> {
        use queries::composio::{GetActiveComposioSlugsForActorQuery, GetComposioSettingsQuery};
        let deployment_id = self.ctx.agent.deployment_id;
        let Ok(thread) = self.ctx.get_thread().await else {
            return Vec::new();
        };
        let actor_id = thread.actor_id;

        let Ok(Some(settings)) = GetComposioSettingsQuery::new(deployment_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
        else {
            return Vec::new();
        };
        if !settings.enabled {
            return Vec::new();
        }
        let enabled_apps: Vec<models::ComposioEnabledApp> =
            serde_json::from_value(settings.enabled_apps).unwrap_or_default();
        let candidate_slugs: Vec<String> = enabled_apps.into_iter().map(|a| a.slug).collect();
        if candidate_slugs.is_empty() {
            return Vec::new();
        }

        GetActiveComposioSlugsForActorQuery::new(deployment_id, actor_id, candidate_slugs)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .unwrap_or_default()
    }

    async fn load_available_sub_agents(&self) -> Vec<dto::json::SubAgentPromptInfo> {
        let Some(sub_agent_ids) = &self.ctx.agent.sub_agents else {
            return Vec::new();
        };
        if sub_agent_ids.is_empty() {
            return Vec::new();
        }

        queries::GetAiAgentsByIdsQuery::new(self.ctx.agent.deployment_id, sub_agent_ids.clone())
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .map(|agents| {
                agents
                    .into_iter()
                    .map(|a| dto::json::SubAgentPromptInfo {
                        name: a.name,
                        description: truncate_prompt_text(
                            a.description,
                            MAX_AGENT_DESCRIPTION_CHARS,
                        ),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    }

    async fn load_active_assignment_prompt_item(
        &self,
    ) -> Result<Option<dto::json::ProjectTaskBoardAssignmentPromptItem>, AppError> {
        let Some(assignment_id) = self.active_thread_event.as_ref().and_then(|event| {
            event
                .assignment_execution_payload()
                .map(|payload| payload.assignment_id)
        }) else {
            return Ok(None);
        };

        Ok(
            GetProjectTaskBoardItemAssignmentByIdQuery::new(assignment_id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await
                .ok()
                .flatten()
                .map(|assignment| {
                    let mut item = Self::assignment_prompt_item_from_row(&assignment);
                    item.mode = Some("assignment_execution".to_string());
                    item
                }),
        )
    }

    async fn load_active_board_item_prompt_item(
        &self,
        active_board_item_id: Option<i64>,
    ) -> Result<Option<dto::json::ProjectTaskBoardPromptItem>, AppError> {
        let Some(item_id) = active_board_item_id else {
            return Ok(None);
        };

        Ok(queries::GetProjectTaskBoardItemByIdQuery::new(item_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .ok()
            .flatten()
            .map(|item| Self::project_task_board_item_to_prompt_item(&item)))
    }

    async fn load_thread_assignment_queue(
        &self,
    ) -> Vec<dto::json::ProjectTaskBoardAssignmentPromptItem> {
        queries::ListAssignmentsForThreadQuery::new(self.ctx.thread_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|assignment| {
                !matches!(
                    assignment.status.as_str(),
                    models::project_task_board::assignment_status::COMPLETED
                        | models::project_task_board::assignment_status::CANCELLED
                        | models::project_task_board::assignment_status::REJECTED
                )
            })
            .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
            .collect()
    }

    async fn load_active_board_item_assignments(
        &self,
        active_board_item_id: Option<i64>,
    ) -> Vec<dto::json::ProjectTaskBoardAssignmentPromptItem> {
        let Some(item_id) = active_board_item_id else {
            return Vec::new();
        };

        queries::ListProjectTaskBoardItemAssignmentsQuery::new(item_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
            .collect()
    }

    fn recent_assignment_history(
        assignments: &[dto::json::ProjectTaskBoardAssignmentPromptItem],
    ) -> Vec<dto::json::ProjectTaskBoardAssignmentPromptItem> {
        if assignments.len() > 5 {
            assignments[assignments.len() - 5..].to_vec()
        } else {
            assignments.to_vec()
        }
    }

    fn scoped_project_task_board_items(
        &self,
        is_coordinator_thread: bool,
        active_board_item: Option<&dto::json::ProjectTaskBoardPromptItem>,
    ) -> Vec<dto::json::ProjectTaskBoardPromptItem> {
        if !is_coordinator_thread {
            return Vec::new();
        }

        active_board_item
            .map(|item| {
                self.project_task_board_items
                    .iter()
                    .filter(|candidate| candidate.task_key == item.task_key)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| self.project_task_board_items.clone())
    }
}
