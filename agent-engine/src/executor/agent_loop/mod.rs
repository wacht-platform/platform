mod meta_tools;
pub(crate) mod prompt;
mod response;
mod tool_schema;
pub(crate) use super::core;

use super::core::AgentExecutor;
use templatekit::{render_template_with_prompt, AgentTemplates};

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use dto::json::agent_executor::ToolCallRequest;
use models::{AgentThreadStatus, ConversationContent, ConversationMessageType};
use queries::{
    GetProjectTaskBoardItemAssignmentByIdQuery, GetProjectTaskScheduleByIdQuery,
    ListPriorScheduleFiresQuery, ListProjectTaskBoardItemCommentsQuery,
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

    async fn handle_notify_user_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
    ) -> Result<bool, AppError> {
        let args: dto::json::agent_executor::NotifyUserParams =
            serde_json::from_value(call.arguments.clone()).map_err(|e| {
                AppError::BadRequest(format!("notify_user params malformed: {e}"))
            })?;
        let message = args.message.trim();
        if message.is_empty() {
            return Err(AppError::BadRequest(
                "notify_user requires a non-empty message".to_string(),
            ));
        }
        let safe_message =
            Self::sanitize_user_facing_message(message, "Posted a status update.");
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
            .with_status(models::AgentThreadStatus::Idle)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        Ok(false)
    }

    async fn handle_resolve_user_feedback_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
    ) -> Result<(), AppError> {
        let args: dto::json::agent_executor::ResolveUserFeedbackParams =
            serde_json::from_value(call.arguments.clone()).map_err(|e| {
                AppError::BadRequest(format!("resolve_user_feedback params malformed: {e}"))
            })?;
        let resolution = args.resolution.trim().to_string();
        if resolution.is_empty() {
            return Err(AppError::BadRequest(
                "resolve_user_feedback requires a non-empty resolution summary".to_string(),
            ));
        }
        let Some(board_item_id) = self.current_board_item_id() else {
            return Err(AppError::BadRequest(
                "resolve_user_feedback can only be called when a board item is active".to_string(),
            ));
        };
        let comment_ids: Vec<i64> = args
            .comment_ids
            .iter()
            .filter_map(|s| s.parse::<i64>().ok())
            .collect();
        if comment_ids.is_empty() {
            return Err(AppError::BadRequest(
                "resolve_user_feedback requires at least one valid comment_id".to_string(),
            ));
        }
        commands::ResolveBoardItemCommentsCommand {
            board_item_id,
            comment_ids,
            resolved_by_thread_id: self.ctx.thread_id,
            resolution_summary: resolution,
        }
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;
        Ok(())
    }

    async fn load_prior_fires_for_board_item(
        &self,
        board_item_id: i64,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        if board_item_id <= 0 {
            return Ok(Vec::new());
        }
        let fires = ListPriorScheduleFiresQuery::new(board_item_id, 5)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Eventual),
            )
            .await?;
        Ok(fires
            .into_iter()
            .map(|f| {
                json!({
                    "task_key": f.task_key,
                    "fired_at": f.fired_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
                    "status": f.status,
                })
            })
            .collect())
    }

    async fn load_comment_timeline_for_board_item(
        &self,
        board_item_id: i64,
    ) -> Result<Vec<serde_json::Value>, AppError> {
        if board_item_id <= 0 {
            return Ok(Vec::new());
        }
        let comments = ListProjectTaskBoardItemCommentsQuery::new(board_item_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Eventual),
            )
            .await?;
        Ok(comments
            .into_iter()
            .map(|c| {
                let attachments = c
                    .metadata
                    .get("attachments")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|a| {
                                a.get("path").and_then(|p| p.as_str()).map(|p| {
                                    json!({
                                        "path": p,
                                        "name": a.get("original_name")
                                            .or_else(|| a.get("name"))
                                            .and_then(|n| n.as_str())
                                            .unwrap_or(""),
                                        "mime_type": a.get("mime_type")
                                            .and_then(|m| m.as_str())
                                            .unwrap_or(""),
                                    })
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                json!({
                    "id": c.id.to_string(),
                    "body": c.body,
                    "created_at": c.created_at.to_rfc3339(),
                    "attachments": attachments,
                    "resolved": c.resolved_at.is_some(),
                    "resolution_summary": c.resolution_summary,
                })
            })
            .collect())
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
                let routing_payload = thread_event.task_routing_payload();
                let parent_task_key_fut = async {
                    match thread_event.board_item_id {
                        Some(id) => queries::GetParentTaskKeyQuery::new(id)
                            .execute_with_db(self.ctx.app_state.db_router.writer())
                            .await,
                        None => Ok(None),
                    }
                };
                let assignments_fut = async {
                    match thread_event.board_item_id {
                        Some(id) => queries::ListProjectTaskBoardItemAssignmentsQuery::new(id)
                            .execute_with_db(self.ctx.app_state.db_router.writer())
                            .await,
                        None => Ok(Vec::new()),
                    }
                };
                let (board_item, parent_task_key, all_assignments) = tokio::try_join!(
                    self.load_board_item_for_thread_event(thread_event, thread_event.board_item_id),
                    parent_task_key_fut,
                    assignments_fut,
                )?;
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
                Self::require_worker_task_identity(thread_event, board_item_id, &task_key, &title)?;
                let workspace_title = board_item
                    .as_ref()
                    .map(|item| item.title.clone())
                    .unwrap_or_else(|| title.clone());
                let is_recurring = board_item
                    .as_ref()
                    .and_then(|item| item.schedule_id)
                    .is_some();
                let carryover = board_item
                    .as_ref()
                    .and_then(|item| item.typed_metadata().schedule_carryover);
                let ((workspace, journal_hash), task_journal_tail) = tokio::try_join!(
                    self.prepare_task_workspace_for_key(&task_key, &workspace_title, is_recurring),
                    self.task_journal_tail_snippet(),
                )?;
                self.initialize_task_journal_start_hash(journal_hash).await?;
                let (schedule_state_pretty, schedule_state_version, schedule_scheduled_for) =
                    Self::format_schedule_carryover(carryover.as_ref());
                self.active_schedule_carryover = carryover;

                let active_assignments = all_assignments
                    .iter()
                    .filter(|a| {
                        matches!(
                            a.status.as_str(),
                            "pending" | "available" | "claimed" | "in_progress",
                        )
                    })
                    .map(|a| {
                        json!({
                            "assignment_id": a.id.to_string(),
                            "assignment_role": a.assignment_role,
                            "thread_id": a.thread_id.to_string(),
                            "status": a.status,
                            "result_status": a.result_status,
                        })
                    })
                    .collect::<Vec<_>>();

                let routing_reason = routing_payload
                    .as_ref()
                    .and_then(|p| p.routing_reason.clone())
                    .unwrap_or_default();
                let previous_status = routing_payload
                    .as_ref()
                    .and_then(|p| p.previous_status.clone());
                let changed_fields = routing_payload
                    .as_ref()
                    .map(|p| p.changed_fields.clone())
                    .unwrap_or_default();
                let last_assignment_result_status = routing_payload
                    .as_ref()
                    .and_then(|p| p.last_assignment_result_status.clone());

                let comment_timeline = self
                    .load_comment_timeline_for_board_item(board_item_id)
                    .await?;
                let prior_fires = self
                    .load_prior_fires_for_board_item(board_item_id)
                    .await?;

                render_template_with_prompt(
                    AgentTemplates::WORKER_TASK_ROUTING_CONTEXT,
                    json!({
                        "task_key": task_key,
                        "board_item_id": board_item_id,
                        "title": title,
                        "description": description,
                        "status": status,
                        "workspace_dir": workspace.directory_path,
                        "task_file": workspace.task_file_path,
                        "journal_file": workspace.journal_file_path,
                        "runbook_file": workspace.runbook_file_path,
                        "task_journal_tail": task_journal_tail,
                        "parent_task_key": parent_task_key,
                        "schedule_state_pretty": schedule_state_pretty,
                        "schedule_state_version": schedule_state_version,
                        "schedule_scheduled_for": schedule_scheduled_for,
                        "routing_reason": routing_reason,
                        "previous_status": previous_status,
                        "changed_fields": changed_fields,
                        "last_assignment_result_status": last_assignment_result_status,
                        "active_assignments": active_assignments,
                        "comment_timeline": comment_timeline,
                        "prior_fires": prior_fires,
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
                let assignment_fut = async {
                    match payload.as_ref().map(|p| p.assignment_id) {
                        Some(assignment_id) => {
                            GetProjectTaskBoardItemAssignmentByIdQuery::new(assignment_id)
                                .execute_with_db(self.ctx.app_state.db_router.writer())
                                .await
                        }
                        None => Ok(None),
                    }
                };
                let parent_task_key_fut = async {
                    match thread_event.board_item_id {
                        Some(id) => queries::GetParentTaskKeyQuery::new(id)
                            .execute_with_db(self.ctx.app_state.db_router.writer())
                            .await,
                        None => Ok(None),
                    }
                };
                let assignments_fut = async {
                    match thread_event.board_item_id {
                        Some(id) => queries::ListProjectTaskBoardItemAssignmentsQuery::new(id)
                            .execute_with_db(self.ctx.app_state.db_router.writer())
                            .await,
                        None => Ok(Vec::new()),
                    }
                };
                let (assignment, board_item, parent_task_key, all_assignments) = tokio::try_join!(
                    assignment_fut,
                    self.load_board_item_for_thread_event(thread_event, thread_event.board_item_id),
                    parent_task_key_fut,
                    assignments_fut,
                )?;
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
                let assignment_id = payload
                    .as_ref()
                    .map(|p| p.assignment_id)
                    .unwrap_or_default();
                Self::require_worker_task_identity(thread_event, board_item_id, &task_key, &title)?;
                let assignment_role = assignment
                    .as_ref()
                    .map(|a| a.assignment_role.as_str())
                    .unwrap_or("executor");
                let instructions = assignment
                    .as_ref()
                    .and_then(|a| a.instructions.as_deref())
                    .unwrap_or("No additional instructions were provided.");
                let handoff_file_path = assignment
                    .as_ref()
                    .and_then(|a| a.result_payload.as_ref())
                    .and_then(|p| p.get("handoff_file_path"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("No handoff file was linked.");
                let is_recurring = board_item
                    .as_ref()
                    .and_then(|item| item.schedule_id)
                    .is_some();
                let carryover = board_item
                    .as_ref()
                    .and_then(|item| item.typed_metadata().schedule_carryover);
                let ((workspace, journal_hash), task_journal_tail) = tokio::try_join!(
                    self.prepare_task_workspace_for_key(&task_key, &title, is_recurring),
                    self.task_journal_tail_snippet(),
                )?;
                self.initialize_task_journal_start_hash(journal_hash).await?;
                let (schedule_state_pretty, schedule_state_version, schedule_scheduled_for) =
                    Self::format_schedule_carryover(carryover.as_ref());
                self.active_schedule_carryover = carryover;

                let current_thread_id = self.ctx.thread_id;
                let prior_assignments_for_thread: Vec<&models::ProjectTaskBoardItemAssignment> =
                    all_assignments
                        .iter()
                        .filter(|a| a.thread_id == current_thread_id && a.id != assignment_id)
                        .collect();
                let assignment_reason = prior_assignments_for_thread
                    .iter()
                    .max_by_key(|a| a.created_at)
                    .map(|prior| match prior.result_status.as_deref() {
                        Some("preempted") => "after_preempt",
                        Some("rejected") => "after_review",
                        _ => "follow_up",
                    })
                    .unwrap_or("first_pickup");
                let active_assignment_count = all_assignments
                    .iter()
                    .filter(|a| {
                        matches!(
                            a.status.as_str(),
                            "pending" | "available" | "claimed" | "in_progress",
                        )
                    })
                    .count();
                let has_reviewer_after = all_assignments.iter().any(|a| {
                    a.id != assignment_id
                        && a.assignment_role == models::project_task_board::assignment_role::REVIEWER
                        && matches!(
                            a.status.as_str(),
                            "pending" | "available" | "claimed" | "in_progress",
                        )
                });

                let comment_timeline = self
                    .load_comment_timeline_for_board_item(board_item_id)
                    .await?;
                let prior_fires = self
                    .load_prior_fires_for_board_item(board_item_id)
                    .await?;

                render_template_with_prompt(
                    AgentTemplates::WORKER_ASSIGNMENT_EXECUTION_CONTEXT,
                    json!({
                        "task_key": task_key,
                        "board_item_id": board_item_id,
                        "title": title,
                        "description": description,
                        "status": status,
                        "assignment_id": assignment_id,
                        "assignment_role": assignment_role,
                        "assignment_count": active_assignment_count,
                        "assignment_reason": assignment_reason,
                        "has_reviewer_after": has_reviewer_after,
                        "instructions": instructions,
                        "handoff_file_path": handoff_file_path,
                        "workspace_dir": workspace.directory_path,
                        "task_file": workspace.task_file_path,
                        "journal_file": workspace.journal_file_path,
                        "runbook_file": workspace.runbook_file_path,
                        "task_journal_tail": task_journal_tail,
                        "parent_task_key": parent_task_key,
                        "schedule_state_pretty": schedule_state_pretty,
                        "schedule_state_version": schedule_state_version,
                        "schedule_scheduled_for": schedule_scheduled_for,
                        "comment_timeline": comment_timeline,
                        "prior_fires": prior_fires,
                    }),
                )
                .map_err(|err| {
                    AppError::Internal(format!(
                        "Failed to render assignment execution context: {}",
                        err
                    ))
                })
            }
            _ => Ok(Self::describe_non_worker_thread_event(thread_event)),
        }
    }

    async fn handle_ask_user_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
    ) -> Result<bool, AppError> {
        use dto::json::ask_user::{validate_question_set, AskUserParams};

        let params: AskUserParams = serde_json::from_value(call.arguments.clone())
            .map_err(|e| AppError::BadRequest(format!("ask_user params malformed: {e}")))?;

        if let Err(e) = validate_question_set(&params.questions) {
            return Err(AppError::BadRequest(format!("ask_user invalid: {e}")));
        }

        self.invalidate_stale_pending_question();
        if self.pending_question.is_some()
            || self.board_item_has_pending_question().await?
        {
            self.store_transient_steer(
                "ask_user_blocked_by_pending_question",
                "I tried to call ask_user, but there is already an active pending question on this thread/task. I must wait for the user to answer the existing question before asking a new one.".to_string(),
            );
            return Ok(true);
        }

        let assignment_id = self
            .active_thread_event
            .as_ref()
            .and_then(|event| event.assignment_execution_payload())
            .map(|payload| payload.assignment_id);
        let pending = models::PendingQuestion {
            questions: params.questions.clone(),
            context: params.context.clone(),
            asked_at: chrono::Utc::now(),
            asked_by_thread_id: self.ctx.thread_id,
            asked_by_assignment_id: assignment_id,
        };

        self.store_conversation(
            ConversationContent::ClarificationRequest {
                questions: serde_json::to_value(&params.questions).unwrap_or_default(),
                context: params.context.clone(),
            },
            ConversationMessageType::ClarificationRequest,
        )
        .await?;

        if let Some(board_item_id) = self.current_board_item_id() {
            commands::SetBoardItemPendingQuestionCommand {
                board_item_id,
                pending_question: Some(pending.clone()),
            }
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        }

        self.pending_question = Some(pending);

        let execution_state = self.build_execution_state_snapshot(None);
        self.apply_thread_status(
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_execution_state(execution_state),
            models::AgentThreadStatus::WaitingForInput,
        )
        .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
        .await?;

        Ok(false)
    }

    fn invalidate_stale_pending_question(&mut self) {
        let Some(pending) = self.pending_question.as_ref() else {
            return;
        };
        let active_assignment_id = self
            .active_thread_event
            .as_ref()
            .and_then(|event| event.assignment_execution_payload())
            .map(|payload| payload.assignment_id);
        if pending.asked_by_assignment_id != active_assignment_id {
            self.pending_question = None;
        }
    }

    async fn board_item_has_pending_question(&self) -> Result<bool, AppError> {
        let Some(board_item_id) = self.current_board_item_id() else {
            return Ok(false);
        };
        let item = queries::GetProjectTaskBoardItemByIdQuery::new(board_item_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?;
        Ok(item
            .and_then(|i| i.pending_question)
            .is_some())
    }

    async fn handle_update_schedule_state(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
    ) -> Result<(), AppError> {
        let Some(carryover) = self.active_schedule_carryover.as_ref() else {
            return Err(AppError::BadRequest(
                "update_schedule_state is only available on recurring task runs".to_string(),
            ));
        };
        let patch = call
            .arguments
            .get("patch")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if !patch.is_object() {
            return Err(AppError::BadRequest(
                "update_schedule_state.patch must be a JSON object".to_string(),
            ));
        }

        let schedule_id = carryover.schedule_id;
        let mut expected_version = carryover.state_version;
        let mut applied = false;
        for _ in 0..3 {
            applied = commands::ApplyScheduleStatePatchCommand::new(
                schedule_id,
                expected_version,
                patch.clone(),
            )
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
            if applied {
                break;
            }
            // Refresh version on conflict and retry.
            let Some(current) = GetProjectTaskScheduleByIdQuery::new(schedule_id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?
            else {
                return Err(AppError::NotFound(format!(
                    "Schedule {schedule_id} not found"
                )));
            };
            expected_version = current.state_version;
        }
        if !applied {
            return Err(AppError::Internal(
                "schedule state update lost CAS race repeatedly".to_string(),
            ));
        }

        if let Some(carryover) = self.active_schedule_carryover.as_mut() {
            if let serde_json::Value::Object(ref obj) = patch {
                if let serde_json::Value::Object(state) = &mut carryover.state_snapshot {
                    for (k, v) in obj {
                        state.insert(k.clone(), v.clone());
                    }
                } else {
                    carryover.state_snapshot = serde_json::Value::Object(obj.clone());
                }
            }
            carryover.state_version = expected_version + 1;
        }
        Ok(())
    }

    /// Formats the schedule carryover snapshot for the worker prompt. Returns
    /// `(state_pretty, state_version, scheduled_for)` as serializable values that
    /// the templates render conditionally; all three are `null` when this is not
    /// a recurring fire.
    fn format_schedule_carryover(
        carryover: Option<&models::ScheduleCarryover>,
    ) -> (
        Option<String>,
        Option<i64>,
        Option<chrono::DateTime<chrono::Utc>>,
    ) {
        let Some(carryover) = carryover else {
            return (None, None, None);
        };
        let pretty = serde_json::to_string_pretty(&carryover.state_snapshot)
            .unwrap_or_else(|_| "{}".to_string());
        (
            Some(pretty),
            Some(carryover.state_version),
            Some(carryover.scheduled_for),
        )
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
        use meta_tools::{
            abort_tool, ask_user_tool, complete_tool, note_tool, notify_user_tool,
            resolve_user_feedback_tool, update_schedule_state_tool,
        };
        use dto::json::agent_executor::{AbortDirective, AbortOutcome};

        let context_json = self.build_agent_loop_context_json().await?;
        let prompt_context: dto::json::AgentLoopPromptEnvelope =
            serde_json::from_value(context_json.clone()).map_err(|e| {
                AppError::Internal(format!("Failed to deserialize prompt context: {e}"))
            })?;
        let request = self.build_agent_loop_request(&prompt_context, &context_json, None)?;

        let available_tools = self.available_tools_for_mode().await;
        let active_board_item = self.active_board_item_prompt_item().await?;
        let mut native_tools: Vec<NativeToolDefinition> = available_tools
            .iter()
            .map(|t| self.build_native_tool_definition(t, active_board_item.as_ref()))
            .collect();
        native_tools.push(note_tool());
        native_tools.push(ask_user_tool());
        if self.terminal_text_nudge_pending {
            native_tools.push(complete_tool());
        }
        if self.is_conversation_thread {
            native_tools.push(notify_user_tool());
        }
        if self.current_board_item_id().is_some() {
            native_tools.push(resolve_user_feedback_tool());
        }
        if self.can_abort_current_assignment_execution() {
            native_tools.push(abort_tool());
        }
        if self.active_schedule_carryover.is_some() {
            native_tools.push(update_schedule_state_tool());
        }

        let llm = self.create_strong_llm().await?;
        let cache_request = self.build_prompt_cache_request().await;
        let output = llm
            .generate_tool_calls(request, native_tools, cache_request)
            .await?;
        self.record_llm_usage_for_compaction(output.usage_metadata.as_ref());
        if let Some(cache_state) = output.cache_state.as_ref() {
            self.write_prompt_cache_state(cache_state).await;
        }

        let note_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "note")
            .cloned()
            .collect();
        let schedule_state_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "update_schedule_state")
            .cloned()
            .collect();
        let ask_user_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "ask_user")
            .cloned()
            .collect();
        let resolve_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "resolve_user_feedback")
            .cloned()
            .collect();
        let notify_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "notify_user")
            .cloned()
            .collect();
        let complete_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "complete")
            .cloned()
            .collect();
        let non_note_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| {
                c.tool_name != "note"
                    && c.tool_name != "update_schedule_state"
                    && c.tool_name != "ask_user"
                    && c.tool_name != "resolve_user_feedback"
                    && c.tool_name != "notify_user"
                    && c.tool_name != "complete"
            })
            .cloned()
            .collect();

        let has_substantive_call = output
            .calls
            .iter()
            .any(|c| c.tool_name != "note" && c.tool_name != "complete");
        if has_substantive_call {
            self.terminal_text_nudge_pending = false;
        }

        if let Some(call) = complete_calls.first() {
            return self.handle_complete_call(call, output.content_text.as_deref()).await;
        }

        for call in &schedule_state_calls {
            self.handle_update_schedule_state(call).await?;
        }

        for call in &resolve_calls {
            self.handle_resolve_user_feedback_call(call).await?;
        }

        if let Some(call) = ask_user_calls.first() {
            return self.handle_ask_user_call(call).await;
        }

        if let Some(call) = notify_calls.first() {
            return self.handle_notify_user_call(call).await;
        }

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
                let count = self.consecutive_note_count;
                self.store_transient_steer(
                    "note_loop_guard",
                    format!(
                        "I've taken {count} notes in a row without making any actual progress — I'm stalling, not thinking. Notes are for anchoring a decision before I act, not a substitute for acting. Next turn I have two productive options: pick the tool that executes the next concrete step, or send the user a final text reply if the work is genuinely done. No more notes until I've moved the work forward."
                    ),
                );
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
            self.store_transient_steer(
                "empty_response_guard",
                "My last turn produced nothing — no tool call, no text. That leaves the work unfinished and the user hanging. I need to converge this turn: if the task is done, send a final text reply to the user that wraps it up. If a concrete step is still required, pick the right tool and call it. Either way, end the indecision now — no more empty turns.".to_string(),
            );
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
            let count = self.repeated_tool_call_count + 1;
            self.store_transient_steer(
                "tool_call_loop_guard",
                format!(
                    "I've called the same tool(s) with the same arguments {count} turns in a row — that's a loop, the outcome isn't going to change by repeating it. The prior tool result already has the answer I need, or it's telling me this approach won't work. I should re-read it, then either change the inputs / pick a different tool / take a different angle, or — if I'm genuinely stuck — reply to the user explaining what I tried and what I need to proceed. The one thing I will not do is fire the identical call again."
                ),
            );
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
            let escape_hint = if self.is_conversation_thread {
                " If I want to hand control back to the user mid-plan without abandoning it (e.g. asking which path to take next), `notify_user` ends the turn cleanly with a status message — the graph stays as-is and resumes when the user replies."
            } else {
                ""
            };
            self.store_transient_steer(
                "complete_blocked_by_task_graph",
                format!(
                    "I tried to wrap up by emitting text only — no tool calls — but the task graph I started still has open nodes. The runtime will keep re-running me until I converge: I can't silently leave a half-executed plan behind. Next turn I either pick the next ready node and run it, or call `task_graph_reset` to abandon the plan cleanly. Then I can finish.{escape_hint}"
                ),
            );
            return Ok(true);
        }

        if self.is_service_mode_execution() && !self.service_mode_journal_was_updated().await? {
            self.store_transient_steer(
                "complete_blocked_by_journal_guard",
                "I tried to wrap up by emitting text only — no tool calls — but `/task/JOURNAL.md` is still empty for this run. The runtime will keep re-running me until I record progress; the coordinator reads the journal to know what I did. Next turn I append a short concrete entry (what I did, what I found, what's left), then finish.".to_string(),
            );
            return Ok(true);
        }

        if !self.allow_complete_for_current_task_owner().await? {
            return Ok(true);
        }

        let safe_message =
            Self::sanitize_user_facing_message(&text, "Completed the requested work.");

        if !self.terminal_text_nudge_pending {
            self.store_conversation(
                ConversationContent::Steer {
                    message: safe_message,
                    further_actions_required: true,
                    reasoning: "Text-only turn — runtime will ask me to confirm before terminating.".to_string(),
                    attachments: None,
                },
                ConversationMessageType::Steer,
            )
            .await?;
            self.store_transient_steer(
                "terminal_text_nudge",
                "My last turn produced text with no tool calls, which the runtime treats as a tentative wrap-up. Before it ends, I get one more pass: re-read my history and decide. If the work is genuinely finished and the text I just sent is the right final answer, I call `complete` with NO additional text — my prior text is already the user-facing answer, any prose alongside `complete` will be duplicated to the user. Just `complete` alone is enough; the runtime ends the turn quietly. Otherwise I keep going — common reasons I might have stopped early: I forgot a step, narrated a tool call as text instead of emitting it, hit a recoverable tool error I should retry or work around, missed something the user asked for, or stopped mid-plan. If any of that applies, I emit the right tool calls now (no `complete`). If I again emit text with no tool calls and no `complete`, the runtime force-terminates with my latest text as the delivery — so this is the round to either confirm (silent `complete`) or correct (emit tool calls).".to_string(),
            );
            self.terminal_text_nudge_pending = true;
            return Ok(true);
        }

        self.terminal_text_nudge_pending = false;
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

    async fn handle_complete_call(
        &mut self,
        call: &crate::llm::GeneratedToolCall,
        _content_text: Option<&str>,
    ) -> Result<bool, AppError> {
        #[derive(serde::Deserialize, Default)]
        struct CompleteArgs {
            #[serde(default)]
            result_summary: Option<String>,
        }
        let args: CompleteArgs = serde_json::from_value(call.arguments.clone()).unwrap_or_default();
        let summary = args
            .result_summary
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or("");
        tracing::info!(
            thread_id = self.ctx.thread_id,
            execution_run_id = self.ctx.execution_run_id,
            result_summary = summary,
            "complete tool acknowledged terminal text response"
        );

        self.terminal_text_nudge_pending = false;

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
