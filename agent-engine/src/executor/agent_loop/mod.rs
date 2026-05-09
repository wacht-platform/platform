mod meta_tools;
pub(crate) mod prompt;
mod response;
mod terminal_review;
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
            serde_json::from_value(call.arguments.clone())
                .map_err(|e| AppError::BadRequest(format!("notify_user params malformed: {e}")))?;
        let message = args.message.trim();
        if message.is_empty() {
            return Err(AppError::BadRequest(
                "notify_user requires a non-empty message".to_string(),
            ));
        }
        let safe_message = Self::sanitize_user_facing_message(message, "Posted a status update.");
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

    async fn unresolved_feedback_ids(&self) -> Result<Vec<i64>, AppError> {
        let Some(board_item_id) = self.current_board_item_id() else {
            return Ok(Vec::new());
        };
        if board_item_id <= 0 {
            return Ok(Vec::new());
        }
        let comments = ListProjectTaskBoardItemCommentsQuery::new(board_item_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?;
        Ok(comments
            .into_iter()
            .filter(|c| c.resolved_at.is_none())
            .map(|c| c.id)
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
                        Some(id) => {
                            queries::GetParentTaskKeyQuery::new(id)
                                .execute_with_db(
                                    self.ctx
                                        .app_state
                                        .db_router
                                        .reader(common::ReadConsistency::Strong),
                                )
                                .await
                        }
                        None => Ok(None),
                    }
                };
                let assignments_fut = async {
                    match thread_event.board_item_id {
                        Some(id) => {
                            queries::ListProjectTaskBoardItemAssignmentsQuery::new(id)
                                .execute_with_db(
                                    self.ctx
                                        .app_state
                                        .db_router
                                        .reader(common::ReadConsistency::Strong),
                                )
                                .await
                        }
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
                self.initialize_task_journal_start_hash(journal_hash)
                    .await?;
                let task_mounts = board_item
                    .as_ref()
                    .map(|item| Self::format_task_mounts(&item.mounts))
                    .unwrap_or_default();
                let schedule_scheduled_for = Self::format_scheduled_for(carryover.as_ref());
                let task_schedule = self
                    .load_schedule_info_for_prompt(
                        board_item.as_ref().and_then(|item| item.schedule_id),
                    )
                    .await?;
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
                let prior_fires = self.load_prior_fires_for_board_item(board_item_id).await?;

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
                        "is_recurring": is_recurring,
                        "task_mounts": task_mounts,
                        "task_schedule": task_schedule,
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
                                .execute_with_db(
                                    self.ctx
                                        .app_state
                                        .db_router
                                        .reader(common::ReadConsistency::Strong),
                                )
                                .await
                        }
                        None => Ok(None),
                    }
                };
                let parent_task_key_fut = async {
                    match thread_event.board_item_id {
                        Some(id) => {
                            queries::GetParentTaskKeyQuery::new(id)
                                .execute_with_db(
                                    self.ctx
                                        .app_state
                                        .db_router
                                        .reader(common::ReadConsistency::Strong),
                                )
                                .await
                        }
                        None => Ok(None),
                    }
                };
                let assignments_fut = async {
                    match thread_event.board_item_id {
                        Some(id) => {
                            queries::ListProjectTaskBoardItemAssignmentsQuery::new(id)
                                .execute_with_db(
                                    self.ctx
                                        .app_state
                                        .db_router
                                        .reader(common::ReadConsistency::Strong),
                                )
                                .await
                        }
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
                self.initialize_task_journal_start_hash(journal_hash)
                    .await?;
                let task_mounts = board_item
                    .as_ref()
                    .map(|item| Self::format_task_mounts(&item.mounts))
                    .unwrap_or_default();
                let schedule_scheduled_for = Self::format_scheduled_for(carryover.as_ref());
                let task_schedule = self
                    .load_schedule_info_for_prompt(
                        board_item.as_ref().and_then(|item| item.schedule_id),
                    )
                    .await?;
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
                        && a.assignment_role
                            == models::project_task_board::assignment_role::REVIEWER
                        && matches!(
                            a.status.as_str(),
                            "pending" | "available" | "claimed" | "in_progress",
                        )
                });

                let comment_timeline = self
                    .load_comment_timeline_for_board_item(board_item_id)
                    .await?;
                let prior_fires = self.load_prior_fires_for_board_item(board_item_id).await?;

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
                        "is_recurring": is_recurring,
                        "task_mounts": task_mounts,
                        "task_schedule": task_schedule,
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
            models::thread_event::event_type::THREAD_SUBSCRIPTION_DELIVERY => {
                self.build_thread_subscription_delivery_message().await
            }
            _ => Ok(Self::describe_non_worker_thread_event(thread_event)),
        }
    }

    async fn build_thread_subscription_delivery_message(&mut self) -> Result<String, AppError> {
        let rows = queries::ListPendingSubscriptionNotificationsQuery::new(self.ctx.thread_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?;

        if rows.is_empty() {
            return Ok("[Task subscription delivery] No pending notifications.".to_string());
        }

        use std::collections::BTreeMap;
        let mut by_task: BTreeMap<String, Vec<&queries::PendingSubscriptionNotification>> =
            BTreeMap::new();
        for row in &rows {
            by_task.entry(row.task_key.clone()).or_default().push(row);
        }

        let mut sections: Vec<String> = Vec::new();
        for (_, entries) in by_task.iter() {
            let first = entries.first().expect("non-empty group");
            let path: Vec<String> = std::iter::once(first.from_status.clone())
                .chain(entries.iter().map(|e| e.to_status.clone()))
                .collect();
            let coalesce_note = if entries.len() > 1 {
                format!(" ({} transitions since last reply)", entries.len())
            } else {
                String::new()
            };
            sections.push(format!(
                "- {} \"{}\": {}{} (latest at {}). Workspace: /project_workspace/tasks/{}",
                first.task_key,
                first.task_title,
                path.join(" → "),
                coalesce_note,
                entries
                    .last()
                    .map(|e| e.transitioned_at.as_str())
                    .unwrap_or(""),
                first.task_key,
            ));
        }

        let consumed = commands::mark_subscription_notifications_consumed(
            self.ctx.app_state.db_router.writer(),
            self.ctx.thread_id,
        )
        .await?;
        tracing::info!(
            thread_id = self.ctx.thread_id,
            count = consumed,
            "marked subscription notifications consumed"
        );

        Ok(format!(
            "[Task subscription delivery]\nYou are subscribed to status changes on the tasks below. Each task's durable state is at the listed `/project_workspace/tasks/<task_key>` path (read TASK.md, JOURNAL.md, artifacts/ as needed). Decide whether to surface this to the user or take action — only mention items they would care about.\n\n{}",
            sections.join("\n")
        ))
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
        if self.pending_question.is_some() || self.board_item_has_pending_question().await? {
            self.store_transient_steer(
                "ask_user_blocked_by_pending_question",
                "ask_user blocked: active pending question already on this thread/task. Wait for user answer before asking new.".to_string(),
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
        Ok(item.and_then(|i| i.pending_question).is_some())
    }

    async fn load_schedule_info_for_prompt(
        &self,
        schedule_id: Option<i64>,
    ) -> Result<Option<serde_json::Value>, AppError> {
        let Some(schedule_id) = schedule_id else {
            return Ok(None);
        };
        let Some(schedule) = GetProjectTaskScheduleByIdQuery::new(schedule_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(json!({
            "kind": schedule.schedule_kind,
            "interval": schedule
                .interval_seconds
                .map(super::project::prompt_items::humanize_interval),
            "next_run_at": schedule.next_run_at.to_rfc3339(),
            "last_fired_at": schedule.last_fired_at.map(|t| t.to_rfc3339()),
            "overlap_policy": schedule.overlap_policy,
        })))
    }

    fn format_task_mounts(mounts: &serde_json::Value) -> Vec<serde_json::Value> {
        models::project_task_schedule::parse_mounts(mounts)
            .unwrap_or_default()
            .into_iter()
            .map(|m| {
                json!({
                    "mount_path": m.mount_path,
                    "mode": m.mode,
                    "description": m.description,
                })
            })
            .collect()
    }

    fn format_scheduled_for(
        carryover: Option<&models::ScheduleCarryover>,
    ) -> Option<chrono::DateTime<chrono::Utc>> {
        carryover.map(|c| c.scheduled_for)
    }

    fn describe_non_worker_thread_event(thread_event: &models::ThreadEvent) -> String {
        format!(
            "Handle queued thread event '{}' for this thread and decide the next action.",
            thread_event.event_type
        )
    }

    #[tracing::instrument(
        name = "agent_loop.run",
        skip(self),
        fields(
            thread_id = self.ctx.thread_id,
            execution_run_id = self.ctx.execution_run_id,
            board_item_id = ?self.current_board_item_id(),
            role = self.current_thread_role().as_str(),
        )
    )]
    pub(super) async fn repl(&mut self) -> Result<(), AppError> {
        use super::hooks::HookKind;
        self.reconcile_agent_skills().await;
        self.run_hooks(HookKind::ExecutionStart).await;
        let result = self.repl_inner().await;
        self.run_hooks(HookKind::ExecutionEnd).await;
        result
    }

    async fn reconcile_agent_skills(&self) {
        let agent_id = self.ctx.agent.id;
        let slugs = match queries::ListAgentSkillsQuery::new(self.ctx.agent.deployment_id, agent_id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Eventual),
            )
            .await
        {
            Ok(rows) => rows.into_iter().map(|r| r.slug).collect::<Vec<_>>(),
            Err(e) => {
                tracing::warn!(agent_id, error = %e, "skills reconcile: list query failed");
                return;
            }
        };
        if let Err(e) = self
            .sandbox
            .reconcile_skills(&agent_id.to_string(), slugs)
            .await
        {
            tracing::warn!(agent_id, error = %e, "skills reconcile: sandbox call failed");
        }
    }

    async fn repl_inner(&mut self) -> Result<(), AppError> {
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

    #[tracing::instrument(
        name = "agent_loop.iteration",
        skip(self),
        fields(
            thread_id = self.ctx.thread_id,
            execution_run_id = self.ctx.execution_run_id,
            board_item_id = ?self.current_board_item_id(),
            iteration = self.current_iteration,
            role = self.current_thread_role().as_str(),
        )
    )]
    async fn run_unified_iteration(&mut self) -> Result<bool, AppError> {
        use crate::llm::NativeToolDefinition;
        use dto::json::agent_executor::{AbortDirective, AbortOutcome};
        use meta_tools::{
            abort_tool, ask_user_tool, note_tool, notify_user_tool, resolve_user_feedback_tool,
        };

        if let Err(exhausted) = self.budget.check() {
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                board_item_id = ?self.current_board_item_id(),
                execution_run_id = self.ctx.execution_run_id,
                "budget exhausted: {}",
                exhausted.reason()
            );
            if self.can_abort_current_assignment_execution() {
                let reason = format!(
                    "Run preempted by budget cap: {}. Coordinator should decide whether to extend, retry with a tighter brief, or mark the task `failed`.",
                    exhausted.reason()
                );
                self.abort_current_assignment_execution(&AbortDirective {
                    reason,
                    outcome: AbortOutcome::Blocked,
                })
                .await?;
            } else {
                self.finish_without_summary().await?;
            }
            return Ok(false);
        }

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
        if self.is_conversation_thread {
            native_tools.push(notify_user_tool());
        }
        if self.current_board_item_id().is_some() {
            native_tools.push(resolve_user_feedback_tool());
        }
        if self.can_abort_current_assignment_execution() {
            native_tools.push(abort_tool());
        }

        let llm = self.create_strong_llm().await?;
        let cache_request = self.build_prompt_cache_request().await;
        let output = llm
            .generate_tool_calls(request, native_tools, cache_request)
            .await?;
        let raw_output_snapshot = serde_json::to_string(&output).unwrap_or_default();
        self.budget.tick_llm();
        self.budget.tick_tools(output.calls.len());
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
        let non_note_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| {
                c.tool_name != "note"
                    && c.tool_name != "ask_user"
                    && c.tool_name != "resolve_user_feedback"
                    && c.tool_name != "notify_user"
            })
            .cloned()
            .collect();

        if output.calls.iter().any(|c| c.tool_name != "note") {
            self.terminal_review_continue_count = 0;
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
                        "{count} notes in a row, no progress. Stalling. Notes anchor decisions, not substitute action. Next turn: pick tool that executes next concrete step, or send final text reply if done. No more notes until work moves."
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
            if !resolve_calls.is_empty() {
                return Ok(true);
            }
            if let Some(text) = output
                .content_text
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty())
            {
                return self.handle_terminal_text_response(text).await;
            }
            let truncated_raw = raw_output_snapshot.chars().take(800).collect::<String>();
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                board_item_id = ?self.current_board_item_id(),
                execution_run_id = self.ctx.execution_run_id,
                raw_output_preview = %truncated_raw,
                "empty_response_guard: LLM returned no tool calls and no text",
            );
            self.store_transient_steer(
                "empty_response_guard",
                "Last turn: nothing — no tool, no text. User left hanging. Converge now: done → final text reply; step left → pick tool and call it. No more empty turns.".to_string(),
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
                        &format!("Tool '{}' arguments must be a JSON object", call.tool_name),
                    )
                    .await?;
                    continue;
                }
            };
            match self.build_tool_call_request_from_native_call(tool, input_object) {
                Ok(req) => tool_requests.push(req),
                Err(e) => {
                    self.record_invalid_tool_call(&call.tool_name, &call.arguments, &e.to_string())
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
            self.repeated_tool_call_count = self.repeated_tool_call_count.saturating_add(1);
        } else {
            self.repeated_tool_call_count = 0;
        }
        self.last_tool_call_signature = Some(signature);

        if self.repeated_tool_call_count >= 2 {
            let count = self.repeated_tool_call_count + 1;
            self.store_transient_steer(
                "tool_call_loop_guard",
                format!(
                    "Same tool(s), same args, {count} turns in a row. Loop. Repeat won't change outcome. Prior result has the answer or says approach won't work — re-read it. Then: change inputs, pick different tool, different angle. Stuck → reply to user with what was tried and what's needed. Identical call again is the one thing forbidden."
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
                let name = r.tool_name();
                if matches!(name, "search_tools" | "load_tools") {
                    return format!("{name}:*");
                }
                let args = r
                    .input_value()
                    .ok()
                    .map(|v| serde_json::to_string(&v).unwrap_or_default())
                    .unwrap_or_default();
                format!("{name}:{args}")
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
                " To hand control to user mid-plan without abandoning it: `notify_user` ends turn cleanly; graph stays, resumes on user reply."
            } else {
                ""
            };
            self.store_transient_steer(
                "complete_blocked_by_task_graph",
                format!(
                    "Tried to wrap up text-only but task graph still has open nodes. Runtime re-runs me until convergence; no silent half-executed plan. Next turn: run the next ready node, or `task_graph_reset` to abandon cleanly. Then finish.{escape_hint}"
                ),
            );
            return Ok(true);
        }

        if self.is_service_mode_execution() && !self.service_mode_journal_was_updated().await? {
            self.store_transient_steer(
                "complete_blocked_by_journal_guard",
                "Tried to wrap up text-only but `/task/JOURNAL.md` still empty this run. Runtime re-runs me until progress recorded; coordinator reads journal. Next turn: append short concrete entry (did/found/left), then finish.".to_string(),
            );
            return Ok(true);
        }

        let unresolved_ids = self.unresolved_feedback_ids().await?;
        if !unresolved_ids.is_empty() {
            let ids_csv = unresolved_ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            self.store_transient_steer(
                "complete_blocked_by_unresolved_feedback",
                format!(
                    "Tried to wrap up text-only but feedback comment(s) {ids_csv} still [unresolved]. Each must be closed via `resolve_user_feedback` (one call per id, with a one-line summary). If you already acted on it, call the tool now. If no action is needed, call it with the explanation. No more text until every id is resolved."
                ),
            );
            return Ok(true);
        }

        if !self.allow_complete_for_current_task_owner().await? {
            return Ok(true);
        }

        let safe_message =
            Self::sanitize_user_facing_message(&text, "Completed the requested work.");

        const MAX_REVIEW_CONTINUES: usize = 2;
        if self.terminal_review_continue_count < MAX_REVIEW_CONTINUES {
            let decision = match self.review_terminal_state(&safe_message).await {
                Ok(d) => d,
                Err(error) => {
                    tracing::warn!(
                        thread_id = self.ctx.thread_id,
                        board_item_id = ?self.current_board_item_id(),
                        execution_run_id = self.ctx.execution_run_id,
                        ?error,
                        "terminal review failed; defaulting to complete"
                    );
                    terminal_review::TerminalReviewDecision {
                        decision: terminal_review::TerminalReviewChoice::Complete,
                        hint: None,
                    }
                }
            };
            tracing::info!(
                thread_id = self.ctx.thread_id,
                board_item_id = ?self.current_board_item_id(),
                execution_run_id = self.ctx.execution_run_id,
                decision = ?decision.decision,
                hint = decision.hint.as_deref().unwrap_or(""),
                "terminal review decision"
            );

            if matches!(
                decision.decision,
                terminal_review::TerminalReviewChoice::Continue
            ) {
                self.terminal_review_continue_count += 1;
                self.store_conversation(
                    ConversationContent::Steer {
                        message: safe_message,
                        further_actions_required: true,
                        reasoning: "Text-only turn — reviewer chose continue.".to_string(),
                        attachments: None,
                    },
                    ConversationMessageType::Steer,
                )
                .await?;
                let hint = decision
                    .hint
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or("concrete unaddressed signal in recent history");
                self.store_transient_steer(
                    "terminal_review_continue",
                    format!(
                        "Internal context for next turn only: {hint}.\n\
                         \n\
                         Act on it directly — emit the tool call(s) that address it, or terminate cleanly if you cannot.\n\
                         \n\
                         Forbidden in your output:\n\
                         - Restating, quoting, or referring to this hint or any \"reviewer\".\n\
                         - Saying things like \"I noticed…\", \"there's an unaddressed…\", \"let me address…\".\n\
                         - Apologizing or acknowledging.\n\
                         \n\
                         The user never sees this message. Just do the work or stop."
                    ),
                );
                return Ok(true);
            }
        }

        self.terminal_review_continue_count = 0;
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
