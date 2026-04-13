mod loop_guard;
mod prompt;
mod response;
mod tool_schema;
pub(crate) use super::core;

use super::core::AgentExecutor;
use crate::template::{render_template_with_prompt, AgentTemplates};

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use dto::json::agent_executor::{
    LoadToolsParams, NextStep, NextStepDecision, SearchToolsParams, ToolCallRequest,
};
use models::{AgentThreadStatus, ConversationContent, ConversationMessageType};
use queries::{
    GetProjectTaskBoardItemAssignmentByIdQuery, ListProjectTaskBoardItemAssignmentsQuery,
};
use serde_json::json;
use std::collections::HashSet;
use tokio::time::{sleep, Duration};

const MAX_LOOP_ITERATIONS: usize = 50;
const LONG_THINK_INPUT_TOKEN_BUDGET: u32 = 2_000_000;
const LONG_THINK_OUTPUT_TOKEN_BUDGET: u32 = 300_000;
pub(super) const STEP_DECISION_CACHE_TTL_SECS: i64 = 20 * 60;
const ACTION_STAGE_LOCAL_RETRY_ATTEMPTS: usize = 8;
const ACTION_STAGE_RETRY_BASE_MS: u64 = 2_000;
const ACTION_STAGE_RETRY_MAX_MS: u64 = 120_000;

impl AgentExecutor {
    async fn persist_active_action_state(
        &mut self,
        directive: Option<dto::json::agent_executor::StartActionDirective>,
        tool_call_brief: Option<dto::json::agent_executor::ToolCallBrief>,
    ) -> Result<(), AppError> {
        if self.active_startaction_directive == directive
            && self.active_tool_call_brief == tool_call_brief
        {
            return Ok(());
        }

        self.active_startaction_directive = directive;
        self.active_tool_call_brief = tool_call_brief;
        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        Ok(())
    }

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

    fn should_retry_action_stage_error(error: &AppError) -> bool {
        matches!(
            error,
            AppError::Internal(_)
                | AppError::Timeout
                | AppError::External(_)
                | AppError::Database(_)
                | AppError::BadRequest(_)
        )
    }

    fn action_stage_retry_delay(attempt: usize) -> Duration {
        let backoff_ms = (ACTION_STAGE_RETRY_BASE_MS
            << ((attempt.saturating_sub(1)).min(7) as u32))
            .min(ACTION_STAGE_RETRY_MAX_MS);
        Duration::from_millis(backoff_ms)
    }

    pub(super) fn should_create_worker_event_message(thread_event: &models::ThreadEvent) -> bool {
        matches!(
            thread_event.event_type.as_str(),
            models::thread_event::event_type::TASK_ROUTING
                | models::thread_event::event_type::ASSIGNMENT_EXECUTION
                | models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW
        )
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

            let decision = match self.decide_next_step().await {
                Ok(decision) => decision,
                Err(e) => {
                    self.handle_loop_error("next-step-decision", e, &mut consecutive_errors)
                        .await?;
                    continue;
                }
            };

            match self.process_decision(decision).await {
                Ok(should_continue) => {
                    consecutive_errors = 0;
                    if !should_continue {
                        return Ok(());
                    }
                }
                Err(e) => {
                    self.handle_loop_error("decision-processing", e, &mut consecutive_errors)
                        .await?;
                }
            }
        }
    }

    async fn process_decision(&mut self, decision: NextStepDecision) -> Result<bool, AppError> {
        let repeated_pattern_count = self.track_decision_pattern(&decision);
        if repeated_pattern_count >= 1 {
            self.refresh_long_think_credits();
            if !self.long_think_mode_active && self.long_think_credits_available() {
                self.long_think_mode_active = true;
            }
        }

        let result = match decision.next_step {
            NextStep::Steer => {
                let last_was_steer = self.conversations.last().map_or(false, |conv| {
                    matches!(conv.message_type, ConversationMessageType::Steer)
                });

                let steer_data = decision.steer.ok_or_else(|| {
                    AppError::Internal("Steer data missing for steer step".to_string())
                })?;

                if steer_data.further_actions_required && last_was_steer {
                    self.store_conversation(
                        ConversationContent::SystemDecision {
                            step: "loop_detection".to_string(),
                            reasoning: "Consecutive steer detected while further work is still required. Previous message was already a steer. Proceeding to gather context or execute action instead.".to_string(),
                            confidence: 1.0,
                        },
                        ConversationMessageType::SystemDecision,
                    ).await?;
                    return Ok(true);
                }

                if !steer_data.further_actions_required {
                    if self.active_task_graph_has_unfinished_nodes()
                        && !self.snapshot_execution_state_requested
                    {
                        let (_graph_status, _pending_nodes, _in_progress_nodes, _failed_nodes) =
                            self.task_graph_snapshot
                                .as_ref()
                                .map(|snapshot| {
                                    let graph_status = snapshot
                                        .get("graph")
                                        .and_then(|graph| graph.get("status"))
                                        .and_then(|status| status.as_str())
                                        .unwrap_or_default()
                                        .to_string();
                                    let nodes = snapshot
                                        .get("nodes")
                                        .and_then(|nodes| nodes.as_array())
                                        .cloned()
                                        .unwrap_or_default();
                                    let pending_nodes =
                                        nodes
                                            .iter()
                                            .filter(|node| {
                                                matches!(
                                            node.get("status").and_then(|status| status.as_str()),
                                            Some(models::thread_task_graph::status::NODE_PENDING)
                                        )
                                            })
                                            .count();
                                    let in_progress_nodes = nodes
                                        .iter()
                                        .filter(|node| {
                                            matches!(
                                            node.get("status").and_then(|status| status.as_str()),
                                            Some(
                                                models::thread_task_graph::status::NODE_IN_PROGRESS
                                            )
                                        )
                                        })
                                        .count();
                                    let failed_nodes =
                                        nodes
                                            .iter()
                                            .filter(|node| {
                                                matches!(
                                            node.get("status").and_then(|status| status.as_str()),
                                            Some(models::thread_task_graph::status::NODE_FAILED)
                                        )
                                            })
                                            .count();
                                    (graph_status, pending_nodes, in_progress_nodes, failed_nodes)
                                })
                                .unwrap_or_else(|| ("missing".to_string(), 0, 0, 0));
                        self.store_conversation(
                            ConversationContent::SystemDecision {
                                step: "complete_blocked_by_task_graph".to_string(),
                                reasoning: "Completion was blocked because the active task graph still has unfinished nodes. Do not send a terminal stop yet. Continue executing ready nodes, or if the graph cannot proceed, write a handoff under `/task/handoffs/` and call `task_graph_mark_failed`. Only conclude after the graph reaches a terminal state.".to_string(),
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

                    self.persist_active_action_state(None, None).await?;
                }

                let safe_steer_message = Self::sanitize_user_facing_message(
                    &steer_data.message,
                    if steer_data.further_actions_required {
                        "Working on it. I will proceed with the request and share updates."
                    } else {
                        "Completed the requested work."
                    },
                );
                let attachments = Self::map_response_attachments(steer_data.attachments.as_ref());
                self.store_conversation(
                    ConversationContent::Steer {
                        message: safe_steer_message,
                        further_actions_required: steer_data.further_actions_required,
                        reasoning: decision.reasoning.clone(),
                        attachments,
                    },
                    ConversationMessageType::Steer,
                )
                .await?;

                if !steer_data.further_actions_required {
                    UpdateAgentThreadStateCommand::new(
                        self.ctx.thread_id,
                        self.ctx.agent.deployment_id,
                    )
                    .with_execution_state(self.build_execution_state_snapshot(None))
                    .with_status(AgentThreadStatus::Idle)
                    .execute_with_deps(
                        &common::deps::from_app(&self.ctx.app_state).db().nats().id(),
                    )
                    .await?;
                }

                Ok(steer_data.further_actions_required)
            }

            NextStep::SearchTools => {
                let directive = decision.search_tools_directive.ok_or_else(|| {
                    AppError::Internal(
                        "Search tools directive is required for searchtools step".to_string(),
                    )
                })?;
                let tool_call = ToolCallRequest::SearchTools {
                    params: SearchToolsParams {
                        queries: directive.queries,
                        max_results_per_query: directive.max_results_per_query,
                    },
                };
                let outcome = self
                    .execute_requested_actions(vec![tool_call.clone()])
                    .await?;
                self.finalize_action_execution_outcome(outcome).await
            }

            NextStep::LoadTools => {
                let directive = decision.load_tools_directive.ok_or_else(|| {
                    AppError::Internal(
                        "Load tools directive is required for loadtools step".to_string(),
                    )
                })?;
                let tool_call = ToolCallRequest::LoadTools {
                    params: LoadToolsParams {
                        tool_names: directive.tool_names,
                    },
                };
                let outcome = self
                    .execute_requested_actions(vec![tool_call.clone()])
                    .await?;
                self.finalize_action_execution_outcome(outcome).await
            }

            NextStep::StartAction => {
                let directive = decision.startaction_directive.as_ref().ok_or_else(|| {
                    AppError::Internal(
                        "Startaction directive is required for startaction step".to_string(),
                    )
                })?;
                self.persist_active_action_state(
                    Some(directive.clone()),
                    directive.tool_call_brief.clone(),
                )
                .await?;
                let outcome = {
                    let mut attempt = 0usize;
                    loop {
                        attempt += 1;
                        match self.execute_action_iteration(directive).await {
                            Ok(outcome) => break outcome,
                            Err(error)
                                if Self::should_retry_action_stage_error(&error)
                                    && attempt < ACTION_STAGE_LOCAL_RETRY_ATTEMPTS =>
                            {
                                sleep(Self::action_stage_retry_delay(attempt)).await;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                };
                self.finalize_action_execution_outcome(outcome).await
            }
            NextStep::ContinueAction => {
                let directive = self.latest_startaction_directive().ok_or_else(|| {
                    AppError::Internal(
                        "Continueaction requires a recent startaction directive".to_string(),
                    )
                })?;
                let continue_guidance = decision
                    .continueaction_directive
                    .as_ref()
                    .map(|d| d.guidance.trim())
                    .filter(|g| !g.is_empty());
                let outcome = {
                    let mut attempt = 0usize;
                    loop {
                        attempt += 1;
                        match self
                            .execute_action_iteration_with_guidance(&directive, continue_guidance)
                            .await
                        {
                            Ok(outcome) => break outcome,
                            Err(error)
                                if Self::should_retry_action_stage_error(&error)
                                    && attempt < ACTION_STAGE_LOCAL_RETRY_ATTEMPTS =>
                            {
                                sleep(Self::action_stage_retry_delay(attempt)).await;
                            }
                            Err(error) => return Err(error),
                        }
                    }
                };
                self.finalize_action_execution_outcome(outcome).await
            }

            NextStep::EnableLongThink => {
                self.refresh_long_think_credits();

                if self.long_think_mode_active {
                    return Err(AppError::BadRequest(
                        "enablelongthink is already active. Choose a concrete next step."
                            .to_string(),
                    ));
                }

                if !self.long_think_credits_available() {
                    return Err(AppError::BadRequest(format!(
                        "enablelongthink has no remaining credits right now. Remaining credits: input {} / {}, output {} / {}.",
                        self.long_think_credit_snapshot.input_tokens_available,
                        LONG_THINK_INPUT_TOKEN_BUDGET,
                        self.long_think_credit_snapshot.output_tokens_available,
                        LONG_THINK_OUTPUT_TOKEN_BUDGET
                    )));
                }

                self.long_think_mode_active = true;
                self.store_conversation(
                    ConversationContent::SystemDecision {
                            step: "long_think_mode_enabled".to_string(),
                        reasoning: format!(
                            "Strong-model mode enabled for the next decision. Remaining credits before use: input {} / {}, output {} / {}. This is accruing rolling credit usage, so get unstuck and step down as soon as possible.",
                            self.long_think_credit_snapshot.input_tokens_available,
                            LONG_THINK_INPUT_TOKEN_BUDGET,
                            self.long_think_credit_snapshot.output_tokens_available,
                            LONG_THINK_OUTPUT_TOKEN_BUDGET
                        ),
                        confidence: decision.confidence as f32,
                    },
                    ConversationMessageType::SystemDecision,
                )
                .await?;

                Ok(true)
            }

            NextStep::Abort => {
                self.persist_active_action_state(None, None).await?;
                let abort_directive = decision.abort_directive.as_ref().ok_or_else(|| {
                    AppError::Internal("Abort data is required for abort step".to_string())
                })?;
                self.abort_current_assignment_execution(abort_directive)
                    .await?;
                Ok(false)
            }
        };

        result
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
