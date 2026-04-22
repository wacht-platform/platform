mod meta_tools;
mod prompt;
mod response;
mod tool_schema;
pub(crate) use super::core;

use super::core::AgentExecutor;
use crate::template::{render_template_with_prompt, AgentTemplates};

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use dto::json::agent_executor::ToolCallRequest;
use models::{AgentThreadStatus, ConversationContent, ConversationMessageType};
use queries::{
    GetProjectTaskBoardItemAssignmentByIdQuery, ListProjectTaskBoardItemAssignmentsQuery,
};
use serde_json::json;
use std::collections::HashSet;

const MAX_LOOP_ITERATIONS: usize = 50;

impl AgentExecutor {
    fn require_worker_task_identity(
        thread_event: &models::ThreadEvent,
        board_item_id: i64,
        task_key: &str,
        title: &str,
    ) -> Result<(), AppError> {
        if board_item_id > 0 && !task_key.trim().is_empty() && !title.trim().is_empty() {
            return Ok(());
        }

        Err(AppError::BadRequest(format!(
            "Invalid worker thread event {} (type={}): missing valid board_item_id/task_key/title",
            thread_event.id, thread_event.event_type
        )))
    }

    pub(super) fn can_abort_current_assignment_execution(&self) -> bool {
        self.active_thread_event
            .as_ref()
            .map(|event| event.event_type == models::thread_event::event_type::ASSIGNMENT_EXECUTION)
            .unwrap_or(false)
    }

    pub(super) async fn build_thread_event_message(
        &mut self,
        thread_event: &models::ThreadEvent,
    ) -> Result<String, AppError> {
        match thread_event.event_type.as_str() {
            models::thread_event::event_type::TASK_ROUTING => {
                let board_item = self
                    .load_board_item_for_thread_event(thread_event, thread_event.board_item_id)
                    .await?;
                let board_item_id = board_item
                    .as_ref()
                    .map(|item| item.id)
                    .or(thread_event.board_item_id)
                    .unwrap_or_default();

                let task_key = board_item
                    .as_ref()
                    .map(|item| item.task_key.clone())
                    .unwrap_or_else(|| Self::fallback_task_key(thread_event, Some(board_item_id)));
                let title = board_item
                    .as_ref()
                    .map(|item| item.title.clone())
                    .unwrap_or_else(|| "Untitled task".to_string());
                let description = board_item
                    .as_ref()
                    .and_then(|item| item.description.clone())
                    .unwrap_or_else(|| "No description provided.".to_string());
                let status = board_item
                    .as_ref()
                    .map(|item| item.status.clone())
                    .unwrap_or_else(|| "pending".to_string());
                let priority = board_item
                    .as_ref()
                    .map(|item| item.priority.clone())
                    .unwrap_or_else(|| "neutral".to_string());
                Self::require_worker_task_identity(thread_event, board_item_id, &task_key, &title)?;
                let workspace = self
                    .ensure_task_workspace_for_key(
                        &task_key,
                        board_item
                            .as_ref()
                            .map(|item| item.title.as_str())
                            .unwrap_or(&title),
                        board_item_id,
                    )
                    .await?;
                let recent_assignments =
                    ListProjectTaskBoardItemAssignmentsQuery::new(board_item_id)
                        .execute_with_db(self.ctx.app_state.db_router.writer())
                        .await?
                        .into_iter()
                        .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
                        .collect::<Vec<_>>();
                let recent_assignments = if recent_assignments.len() > 5 {
                    recent_assignments[recent_assignments.len() - 5..].to_vec()
                } else {
                    recent_assignments
                };
                let recent_assignment_history_summary =
                    Self::summarize_assignment_list(&recent_assignments, 5);
                let task_journal_tail = self.task_journal_tail_snippet().await?;

                render_template_with_prompt(
                    AgentTemplates::WORKER_TASK_ROUTING_CONTEXT,
                    json!({
                        "task_key": task_key,
                        "board_item_id": board_item_id,
                        "title": title,
                        "description": description,
                        "status": status,
                        "priority": priority,
                        "workspace_dir": workspace.directory_path,
                        "task_file": workspace.task_file_path,
                        "journal_file": workspace.journal_file_path,
                        "runbook_file": workspace.runbook_file_path,
                        "recent_assignments": recent_assignments,
                        "recent_assignment_history_summary": recent_assignment_history_summary,
                        "task_journal_tail": task_journal_tail,
                    }),
                )
                .map_err(|err| {
                    AppError::Internal(format!(
                        "Failed to render worker task routing context: {}",
                        err
                    ))
                })
            }
            models::thread_event::event_type::ASSIGNMENT_EXECUTION => {
                let payload = thread_event.assignment_execution_payload();
                let assignment =
                    if let Some(assignment_id) = payload.as_ref().map(|p| p.assignment_id) {
                        GetProjectTaskBoardItemAssignmentByIdQuery::new(assignment_id)
                            .execute_with_db(self.ctx.app_state.db_router.writer())
                            .await?
                    } else {
                        None
                    };
                let board_item = self
                    .load_board_item_for_thread_event(thread_event, thread_event.board_item_id)
                    .await?;
                let board_item_id = board_item
                    .as_ref()
                    .map(|item| item.id)
                    .or(thread_event.board_item_id)
                    .unwrap_or_default();
                let task_key = board_item
                    .as_ref()
                    .map(|item| item.task_key.clone())
                    .unwrap_or_else(|| Self::fallback_task_key(thread_event, Some(board_item_id)));
                let title = board_item
                    .as_ref()
                    .map(|item| item.title.clone())
                    .unwrap_or_else(|| "Untitled task".to_string());
                let description = board_item
                    .as_ref()
                    .and_then(|item| item.description.clone())
                    .unwrap_or_else(|| "No description provided.".to_string());
                let status = board_item
                    .as_ref()
                    .map(|item| item.status.clone())
                    .unwrap_or_else(|| "pending".to_string());
                let priority = board_item
                    .as_ref()
                    .map(|item| item.priority.clone())
                    .unwrap_or_else(|| "neutral".to_string());
                let assignment_id = payload
                    .as_ref()
                    .map(|p| p.assignment_id)
                    .unwrap_or_default();
                Self::require_worker_task_identity(thread_event, board_item_id, &task_key, &title)?;
                let assignment_role = assignment
                    .as_ref()
                    .map(|a| a.assignment_role.as_str())
                    .unwrap_or("executor");
                let assignment_order = assignment.as_ref().map(|a| a.assignment_order).unwrap_or(1);
                let instructions = assignment
                    .as_ref()
                    .and_then(|a| a.instructions.as_deref())
                    .unwrap_or("No additional instructions were provided.");
                let handoff_file_path = assignment
                    .as_ref()
                    .and_then(|a| a.handoff_file_path.as_deref())
                    .unwrap_or("No handoff file was linked.");
                let workspace = self
                    .ensure_task_workspace_for_key(&task_key, &title, board_item_id)
                    .await?;
                let recent_assignments =
                    ListProjectTaskBoardItemAssignmentsQuery::new(board_item_id)
                        .execute_with_db(self.ctx.app_state.db_router.writer())
                        .await?
                        .into_iter()
                        .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
                        .collect::<Vec<_>>();
                let recent_assignments = if recent_assignments.len() > 5 {
                    recent_assignments[recent_assignments.len() - 5..].to_vec()
                } else {
                    recent_assignments
                };
                let recent_assignment_history_summary =
                    Self::summarize_assignment_list(&recent_assignments, 5);
                let task_journal_tail = self.task_journal_tail_snippet().await?;

                render_template_with_prompt(
                    AgentTemplates::WORKER_ASSIGNMENT_EXECUTION_CONTEXT,
                    json!({
                        "task_key": task_key,
                        "board_item_id": board_item_id,
                        "title": title,
                        "description": description,
                        "status": status,
                        "priority": priority,
                        "assignment_id": assignment_id,
                        "assignment_role": assignment_role,
                        "assignment_order": assignment_order,
                        "instructions": instructions,
                        "handoff_file_path": handoff_file_path,
                        "workspace_dir": workspace.directory_path,
                        "task_file": workspace.task_file_path,
                        "journal_file": workspace.journal_file_path,
                        "runbook_file": workspace.runbook_file_path,
                        "recent_assignments": recent_assignments,
                        "recent_assignment_history_summary": recent_assignment_history_summary,
                        "task_journal_tail": task_journal_tail,
                    }),
                )
                .map_err(|err| {
                    AppError::Internal(format!(
                        "Failed to render assignment execution context: {}",
                        err
                    ))
                })
            }
            models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW => {
                let payload = thread_event.assignment_outcome_review_payload();
                let assignment =
                    if let Some(assignment_id) = payload.as_ref().map(|p| p.assignment_id) {
                        GetProjectTaskBoardItemAssignmentByIdQuery::new(assignment_id)
                            .execute_with_db(self.ctx.app_state.db_router.writer())
                            .await?
                    } else {
                        None
                    };
                let board_item = self
                    .load_board_item_for_thread_event(thread_event, thread_event.board_item_id)
                    .await?;
                let board_item_id = board_item
                    .as_ref()
                    .map(|item| item.id)
                    .or(thread_event.board_item_id)
                    .unwrap_or_default();
                let task_key = board_item
                    .as_ref()
                    .map(|item| item.task_key.clone())
                    .unwrap_or_else(|| Self::fallback_task_key(thread_event, Some(board_item_id)));
                let title = board_item
                    .as_ref()
                    .map(|item| item.title.clone())
                    .unwrap_or_else(|| "Untitled task".to_string());
                let description = board_item
                    .as_ref()
                    .and_then(|item| item.description.clone())
                    .unwrap_or_else(|| "No description provided.".to_string());
                let status = board_item
                    .as_ref()
                    .map(|item| item.status.clone())
                    .unwrap_or_else(|| "pending".to_string());
                let priority = board_item
                    .as_ref()
                    .map(|item| item.priority.clone())
                    .unwrap_or_else(|| "neutral".to_string());
                let assignment_id = payload
                    .as_ref()
                    .map(|p| p.assignment_id)
                    .unwrap_or_default();
                Self::require_worker_task_identity(thread_event, board_item_id, &task_key, &title)?;
                let assignment_role = assignment
                    .as_ref()
                    .map(|a| a.assignment_role.as_str())
                    .unwrap_or("executor");
                let result_status = assignment
                    .as_ref()
                    .and_then(|a| a.result_status.as_deref())
                    .unwrap_or_else(|| {
                        assignment
                            .as_ref()
                            .map(|a| a.status.as_str())
                            .unwrap_or("unknown")
                    });
                let result_summary = assignment
                    .as_ref()
                    .and_then(|a| a.result_summary.as_deref())
                    .unwrap_or("No summary was returned.");
                let handoff_file_path = assignment
                    .as_ref()
                    .and_then(|a| a.handoff_file_path.as_deref())
                    .unwrap_or("No handoff file was linked.");
                let workspace = self
                    .ensure_task_workspace_for_key(&task_key, &title, board_item_id)
                    .await?;
                let recent_assignments =
                    ListProjectTaskBoardItemAssignmentsQuery::new(board_item_id)
                        .execute_with_db(self.ctx.app_state.db_router.writer())
                        .await?
                        .into_iter()
                        .map(|assignment| Self::assignment_prompt_item_from_row(&assignment))
                        .collect::<Vec<_>>();
                let recent_assignments = if recent_assignments.len() > 5 {
                    recent_assignments[recent_assignments.len() - 5..].to_vec()
                } else {
                    recent_assignments
                };
                let recent_assignment_history_summary =
                    Self::summarize_assignment_list(&recent_assignments, 5);
                let task_journal_tail = self.task_journal_tail_snippet().await?;

                render_template_with_prompt(
                    AgentTemplates::WORKER_ASSIGNMENT_OUTCOME_REVIEW_CONTEXT,
                    json!({
                        "task_key": task_key,
                        "board_item_id": board_item_id,
                        "title": title,
                        "description": description,
                        "status": status,
                        "priority": priority,
                        "assignment_id": assignment_id,
                        "assignment_role": assignment_role,
                        "result_status": result_status,
                        "result_summary": result_summary,
                        "handoff_file_path": handoff_file_path,
                        "workspace_dir": workspace.directory_path,
                        "task_file": workspace.task_file_path,
                        "journal_file": workspace.journal_file_path,
                        "runbook_file": workspace.runbook_file_path,
                        "recent_assignments": recent_assignments,
                        "recent_assignment_history_summary": recent_assignment_history_summary,
                        "task_journal_tail": task_journal_tail,
                    }),
                )
                .map_err(|err| {
                    AppError::Internal(format!(
                        "Failed to render assignment outcome review context: {}",
                        err
                    ))
                })
            }
            _ => Ok(Self::describe_non_worker_thread_event(thread_event)),
        }
    }

    fn describe_non_worker_thread_event(thread_event: &models::ThreadEvent) -> String {
        format!(
            "Handle queued thread event '{}' for this thread and decide the next action.",
            thread_event.event_type
        )
    }

    pub(super) async fn repl(&mut self) -> Result<(), AppError> {
        let mut iteration = 0;
        let mut consecutive_errors = 0usize;
        loop {
            iteration += 1;
            self.current_iteration = iteration;

            if iteration > MAX_LOOP_ITERATIONS {
                self.finish_without_summary().await?;
                return Ok(());
            }

            match self.run_unified_iteration().await {
                Ok(true) => {
                    consecutive_errors = 0;
                }
                Ok(false) => {
                    return Ok(());
                }
                Err(e) => {
                    self.handle_loop_error("unified-iteration", e, &mut consecutive_errors)
                        .await?;
                }
            }
        }
    }

    async fn run_unified_iteration(&mut self) -> Result<bool, AppError> {
        use crate::llm::NativeToolDefinition;
        use meta_tools::{abort_tool, note_tool};
        use dto::json::agent_executor::{AbortDirective, AbortOutcome};

        let context_json = self.build_agent_loop_context_json().await?;
        let prompt_context: dto::json::AgentLoopPromptEnvelope =
            serde_json::from_value(context_json.clone()).map_err(|e| {
                AppError::Internal(format!("Failed to deserialize prompt context: {e}"))
            })?;
        let request = self.build_agent_loop_request(
            &prompt_context,
            &context_json,
            None,
        )?;

        let available_tools = self.available_tools_for_mode().await;
        let active_board_item = self.active_board_item_prompt_item().await?;
        let mut native_tools: Vec<NativeToolDefinition> = available_tools
            .iter()
            .map(|t| self.build_native_tool_definition(t, active_board_item.as_ref()))
            .collect();
        native_tools.push(note_tool());
        if self.can_abort_current_assignment_execution() {
            native_tools.push(abort_tool());
        }

        let llm = self.create_strong_llm().await?;
        let output = llm.generate_tool_calls(request, native_tools).await?;
        self.record_llm_usage_for_compaction(output.usage_metadata.as_ref());

        let note_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "note")
            .cloned()
            .collect();
        let non_note_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name != "note")
            .cloned()
            .collect();

        if !note_calls.is_empty() {
            for call in &note_calls {
                let entry = call
                    .arguments
                    .get("entry")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                if entry.is_empty() {
                    continue;
                }
                self.store_conversation(
                    ConversationContent::SystemDecision {
                        step: "note".to_string(),
                        reasoning: entry,
                        confidence: 1.0,
                    },
                    ConversationMessageType::SystemDecision,
                )
                .await?;
            }
        }

        let note_only = !note_calls.is_empty()
            && non_note_calls.is_empty()
            && output
                .content_text
                .as_ref()
                .map(|t| t.trim().is_empty())
                .unwrap_or(true);

        if note_only {
            self.consecutive_note_count = self.consecutive_note_count.saturating_add(1);
            if self.consecutive_note_count >= 3 {
                self.store_conversation(
                    ConversationContent::SystemDecision {
                        step: "note_loop_guard".to_string(),
                        reasoning: format!(
                            "You have taken {} notes in a row without making progress. On the next turn: either call a real work tool to act on your plan, or respond to the user with text. No more notes until you have acted.",
                            self.consecutive_note_count
                        ),
                        confidence: 1.0,
                    },
                    ConversationMessageType::SystemDecision,
                )
                .await?;
            }
            return Ok(true);
        } else {
            self.consecutive_note_count = 0;
        }

        if let Some(abort_call) = non_note_calls.iter().find(|c| c.tool_name == "abort_task") {
            #[derive(serde::Deserialize)]
            struct AbortArgs {
                outcome: AbortOutcome,
                reason: String,
            }
            let args: AbortArgs = serde_json::from_value(abort_call.arguments.clone())
                .map_err(|e| AppError::Internal(format!("abort_task args malformed: {e}")))?;
            self.abort_current_assignment_execution(&AbortDirective {
                outcome: args.outcome,
                reason: args.reason,
            })
            .await?;
            return Ok(false);
        }

        if non_note_calls.is_empty() {
            if let Some(text) = output
                .content_text
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
            {
                return self.handle_terminal_text_response(text).await;
            }
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "empty_response_guard".to_string(),
                    reasoning: "Your last turn emitted no tool calls and no text. Every turn must either call at least one tool or emit a final text response to the user. Try again.".to_string(),
                    confidence: 1.0,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;
            return Ok(true);
        }

        if let Some(text) = output
            .content_text
            .as_ref()
            .map(|t| t.trim())
            .filter(|t| !t.is_empty())
        {
            self.store_conversation(
                ConversationContent::Steer {
                    message: Self::sanitize_user_facing_message(
                        text,
                        "Working on it. I will proceed with the request and share updates.",
                    ),
                    further_actions_required: true,
                    reasoning: "Progress note emitted alongside tool calls.".to_string(),
                    attachments: None,
                },
                ConversationMessageType::Steer,
            )
            .await?;
        }

        let mut tool_requests: Vec<ToolCallRequest> = Vec::new();
        for call in non_note_calls
            .into_iter()
            .filter(|c| c.tool_name != "abort_task")
        {
            let tool = match available_tools.iter().find(|t| t.name == call.tool_name) {
                Some(t) => t,
                None => {
                    self.record_invalid_tool_call(
                        &call.tool_name,
                        &call.arguments,
                        &format!("Model selected unknown tool '{}'", call.tool_name),
                    )
                    .await?;
                    continue;
                }
            };
            let input_object = match call.arguments.as_object().cloned() {
                Some(obj) => obj,
                None => {
                    self.record_invalid_tool_call(
                        &call.tool_name,
                        &call.arguments,
                        &format!(
                            "Tool '{}' arguments must be a JSON object",
                            call.tool_name
                        ),
                    )
                    .await?;
                    continue;
                }
            };
            match self.build_tool_call_request_from_native_call(tool, input_object) {
                Ok(req) => tool_requests.push(req),
                Err(e) => {
                    self.record_invalid_tool_call(
                        &call.tool_name,
                        &call.arguments,
                        &e.to_string(),
                    )
                    .await?;
                }
            }
        }

        if tool_requests.is_empty() {
            return Ok(true);
        }

        let signature = Self::tool_call_signature(&tool_requests);
        if self
            .last_tool_call_signature
            .as_deref()
            .map(|prev| prev == signature)
            .unwrap_or(false)
        {
            self.repeated_tool_call_count =
                self.repeated_tool_call_count.saturating_add(1);
        } else {
            self.repeated_tool_call_count = 0;
        }
        self.last_tool_call_signature = Some(signature);

        if self.repeated_tool_call_count >= 2 {
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "tool_call_loop_guard".to_string(),
                    reasoning: format!(
                        "You have called the same tool(s) with the same arguments {} turns in a row. This is a loop — the outcome will not change. Read the prior tool result(s) carefully, change your approach, or respond to the user if you are stuck. Do not repeat the identical call again.",
                        self.repeated_tool_call_count + 1
                    ),
                    confidence: 1.0,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;
            if self.repeated_tool_call_count >= 4 {
                return Ok(true);
            }
        }

        let outcome = self.execute_requested_actions(tool_requests).await?;
        self.finalize_action_execution_outcome(outcome).await
    }

    fn tool_call_signature(requests: &[ToolCallRequest]) -> String {
        let mut parts: Vec<String> = requests
            .iter()
            .map(|r| {
                let args = r
                    .input_value()
                    .ok()
                    .map(|v| serde_json::to_string(&v).unwrap_or_default())
                    .unwrap_or_default();
                format!("{}:{}", r.tool_name(), args)
            })
            .collect();
        parts.sort();
        parts.join("|")
    }

    async fn record_invalid_tool_call(
        &mut self,
        tool_name: &str,
        arguments: &serde_json::Value,
        error: &str,
    ) -> Result<(), AppError> {
        self.store_conversation(
            ConversationContent::ToolResult {
                tool_name: tool_name.to_string(),
                status: "error".to_string(),
                input: arguments.clone(),
                output: None,
                error: Some(error.to_string()),
            },
            ConversationMessageType::ToolResult,
        )
        .await
    }

    async fn handle_terminal_text_response(&mut self, text: String) -> Result<bool, AppError> {
        if self.active_task_graph_has_unfinished_nodes() {
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "complete_blocked_by_task_graph".to_string(),
                    reasoning: "Completion was blocked because the active task graph still has unfinished nodes. Continue executing ready nodes, complete or fail them, or call `task_graph_reset` if the whole plan needs to be abandoned. Only conclude after the graph reaches a terminal state.".to_string(),
                    confidence: 1.0,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;
            return Ok(true);
        }

        if self.is_service_mode_execution() && !self.service_mode_journal_was_updated().await? {
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "complete_blocked_by_journal_guard".to_string(),
                    reasoning: "Completion was blocked because /task/JOURNAL.md has not been updated during this run. Service-mode assignments must record progress in the journal before finishing. Write or edit /task/JOURNAL.md, then finish.".to_string(),
                    confidence: 1.0,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;
            return Ok(true);
        }

        if !self.allow_complete_for_current_task_owner().await? {
            return Ok(true);
        }

        let safe_message =
            Self::sanitize_user_facing_message(&text, "Completed the requested work.");
        self.store_conversation(
            ConversationContent::Steer {
                message: safe_message,
                further_actions_required: false,
                reasoning: "Terminal text response — no further tool calls emitted.".to_string(),
                attachments: None,
            },
            ConversationMessageType::Steer,
        )
        .await?;

        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .with_status(AgentThreadStatus::Idle)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        Ok(false)
    }


    pub(super) fn derive_input_safety_signals(&self) -> Vec<String> {
        let Some((source, latest_input)) =
            self.conversations
                .iter()
                .rev()
                .find_map(|conv| match &conv.content {
                    ConversationContent::UserMessage { message, .. } => {
                        Some(("user_message", message.as_str()))
                    }
                    _ => None,
                })
        else {
            return Vec::new();
        };

        let input_lower = latest_input.to_lowercase();
        let mut seen = HashSet::new();
        let mut signals = Vec::new();

        let pattern_checks = [
            (
                "instruction_override",
                "Attempt to override system rules detected",
                &[
                    "ignore previous instructions",
                    "disregard prior instructions",
                    "forget all rules",
                    "override system prompt",
                ][..],
            ),
            (
                "prompt_exfiltration",
                "Attempt to reveal hidden prompts or internal policy detected",
                &[
                    "show system prompt",
                    "reveal your prompt",
                    "print your instructions",
                    "developer instructions",
                ][..],
            ),
            (
                "safety_bypass",
                "Attempt to bypass safety constraints detected",
                &[
                    "disable safety",
                    "jailbreak",
                    "bypass policy",
                    "no restrictions",
                ][..],
            ),
            (
                "secret_exfiltration",
                "Request may involve secrets, credentials, or token exfiltration",
                &[
                    "api key",
                    "access token",
                    "password",
                    "private key",
                    "secret",
                ][..],
            ),
            (
                "destructive_operations",
                "Potential destructive operation request detected",
                &[
                    "drop database",
                    "delete all",
                    "rm -rf",
                    "truncate table",
                    "wipe",
                ][..],
            ),
        ];

        for (tag, message, phrases) in pattern_checks {
            if phrases.iter().any(|phrase| input_lower.contains(phrase)) && seen.insert(tag) {
                signals.push(format!("[{}] {}", source, message));
            }
        }

        if signals.len() > 6 {
            signals.truncate(6);
        }

        signals
    }
}
