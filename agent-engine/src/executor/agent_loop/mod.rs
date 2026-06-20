pub(crate) mod meta_tools;
pub(crate) mod prompt;
mod response;
mod shell_guard;
mod tool_call_salvage;
mod tool_schema;
pub(crate) use super::core;

use super::core::AgentExecutor;
use crate::llm::NativeToolDefinition;
use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use common::ResultExt;
use dto::json::agent_executor::ToolCallRequest;
use dto::json::agent_executor::{AbortDirective, AbortOutcome};
use meta_tools::{
    abort_tool, ask_user_tool, complete_tool, note_tool, notify_user_tool,
    resolve_user_feedback_tool,
};
use models::{AgentThreadStatus, ConversationContent, ConversationMessageType};
use queries::{
    GetProjectTaskBoardItemAssignmentByIdQuery, GetProjectTaskScheduleByIdQuery,
    ListPriorScheduleFiresQuery, ListProjectTaskBoardItemCommentsQuery,
};
use serde_json::json;
use std::collections::HashSet;
use templatekit::{render_template_with_prompt, AgentTemplates};

const MAX_LOOP_ITERATIONS: usize = 300;
const MAX_UNPRODUCTIVE_TURNS: usize = 4;
const LARGE_TOOL_BATCH: usize = 10;
const SHELL_NUDGE_ESCALATE_AT: usize = 2;

/// Meta tools drive the loop, not the task — excluded from the work-tool budget.
fn is_meta_tool_name(name: &str) -> bool {
    matches!(
        name,
        "note"
            | "ask_user"
            | "terminate_loop"
            | "notify_user"
            | "resolve_user_feedback"
            | "abort_task"
    )
}

fn detect_leaked_tool_call(text: &str) -> bool {
    text.contains("<tool_call>")
        || text.contains("</tool_call>")
        || (text.contains("<function=") && text.contains("<parameter="))
        || text.contains("[TOOL_CALLS]")
        || text.contains("<|python_tag|>")
        || text.contains("<｜tool▁call")
        || text.contains("<|START_ACTION|>")
        || (text.contains("<arg_key>") && text.contains("<arg_value>"))
}

/// Provider finish reasons that mean the output was cut off at the token limit
/// (OpenAI/OpenRouter "length", Gemini "MAX_TOKENS"), not a clean stop.
fn is_truncated_finish(reason: &str) -> bool {
    reason.eq_ignore_ascii_case("length") || reason.eq_ignore_ascii_case("max_tokens")
}

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
    ) -> Result<Vec<dto::json::CommentTimelinePromptItem>, AppError> {
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
        let timeline_state: Vec<String> = comments
            .iter()
            .map(|c| {
                format!(
                    "{}={}",
                    c.id,
                    if c.resolved_at.is_some() {
                        "resolved"
                    } else {
                        "unresolved"
                    }
                )
            })
            .collect();
        tracing::debug!(
            target: "loop",
            board_item_id,
            timeline = ?timeline_state,
            "comment_timeline loaded"
        );
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
                                    dto::json::CommentAttachmentPromptItem {
                                        path: p.to_string(),
                                        name: a
                                            .get("original_name")
                                            .or_else(|| a.get("name"))
                                            .and_then(|n| n.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        mime_type: a
                                            .get("mime_type")
                                            .and_then(|m| m.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                    }
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                dto::json::CommentTimelinePromptItem {
                    id: c.id.to_string(),
                    body: c.body,
                    created_at: c.created_at.to_rfc3339(),
                    resolved: c.resolved_at.is_some(),
                    attachments,
                    resolution_summary: c.resolution_summary,
                }
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
        let ids: Vec<i64> = comments
            .into_iter()
            .filter(|c| c.resolved_at.is_none())
            .map(|c| c.id)
            .collect();
        Ok(ids)
    }

    pub(super) fn can_abort_current_assignment_execution(&self) -> bool {
        self.active_thread_event
            .as_ref()
            .map(|event| event.event_type == models::thread_event::event_type::ASSIGNMENT_EXECUTION)
            .unwrap_or(false)
    }

    /// `abort_task` blocks/cancels the assignment and stalls the board item, so
    /// it is a last resort — not a routine exit. A progressing run ends cleanly
    /// via `terminate_loop` (success) or `update_project_task(blocked)` +
    /// `terminate_loop` (hand a block back without cancelling the task graph). We
    /// only surface abort once the run has tried to exit cleanly and been refused
    /// at least twice — i.e. it genuinely cannot get out by other means.
    pub(super) fn should_offer_abort_task(&self) -> bool {
        self.can_abort_current_assignment_execution() && self.terminate_loop_guard_rejections >= 2
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
                    self.prepare_task_workspace_for_key(
                        &task_key,
                        &workspace_title,
                        is_recurring,
                        board_item.as_ref(),
                    ),
                    self.task_journal_tail_snippet(),
                )?;
                self.initialize_task_journal_start_hash(journal_hash)
                    .await?;
                let task_mounts = board_item
                    .as_ref()
                    .map(|item| Self::format_task_mounts(&item.mounts))
                    .unwrap_or_default();
                let task_attachments = board_item
                    .as_ref()
                    .map(Self::format_task_attachments)
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
                        "task_journal_tail": task_journal_tail,
                        "parent_task_key": parent_task_key,
                        "is_recurring": is_recurring,
                        "task_attachments": task_attachments,
                        "task_mounts": task_mounts,
                        "task_schedule": task_schedule,
                        "schedule_scheduled_for": schedule_scheduled_for,
                        "routing_reason": routing_reason,
                        "previous_status": previous_status,
                        "changed_fields": changed_fields,
                        "last_assignment_result_status": last_assignment_result_status,
                        "active_assignments": active_assignments,
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
                let is_delegated = board_item
                    .as_ref()
                    .map(|item| item.exclusive_owner_agent_id.is_some())
                    .unwrap_or(false);
                let delegated_by_thread_id = board_item.as_ref().and_then(|item| {
                    item.metadata
                        .get("delegated_by_thread_id")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                });
                let delegated_workspace_mount = if is_delegated {
                    board_item.as_ref().and_then(|item| {
                        item.mounts.as_array().and_then(|arr| {
                            arr.iter().find_map(|m| {
                                let path = m.get("mount_path").and_then(|v| v.as_str())?;
                                if path == "/delegated_workspace" {
                                    Some(path.to_string())
                                } else {
                                    None
                                }
                            })
                        })
                    })
                } else {
                    None
                };
                let delegated_input_mounts = if is_delegated {
                    board_item
                        .as_ref()
                        .and_then(|item| item.mounts.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|m| {
                                    let path = m.get("mount_path").and_then(|v| v.as_str())?;
                                    if !path.starts_with("/delegated_inputs/") {
                                        return None;
                                    }
                                    Some(serde_json::json!({
                                        "mount_path": path,
                                        "alias": m.get("alias").and_then(|v| v.as_str()),
                                        "source_path": m.get("source_path").and_then(|v| v.as_str()),
                                    }))
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                let carryover = board_item
                    .as_ref()
                    .and_then(|item| item.typed_metadata().schedule_carryover);
                let ((workspace, journal_hash), task_journal_tail) = tokio::try_join!(
                    self.prepare_task_workspace_for_key(
                        &task_key,
                        &title,
                        is_recurring,
                        board_item.as_ref(),
                    ),
                    self.task_journal_tail_snippet(),
                )?;
                self.initialize_task_journal_start_hash(journal_hash)
                    .await?;
                let task_mounts = board_item
                    .as_ref()
                    .map(|item| Self::format_task_mounts(&item.mounts))
                    .unwrap_or_default();
                let task_attachments = board_item
                    .as_ref()
                    .map(Self::format_task_attachments)
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

                // Comments/feedback are the coordinator's task-level concern, not
                // the executor's. Executors must not fetch or be shown task
                // comments — it made them try to resolve_user_feedback they don't
                // own (UPDATE hits 0 rows) and loop. The coordinator bakes any
                // feedback the executor must act on into the assignment brief.
                let comment_timeline: Vec<serde_json::Value> = Vec::new();
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
                        "task_journal_tail": task_journal_tail,
                        "parent_task_key": parent_task_key,
                        "is_recurring": is_recurring,
                        "is_delegated": is_delegated,
                        "delegated_by_thread_id": delegated_by_thread_id,
                        "delegated_workspace_mount": delegated_workspace_mount,
                        "delegated_input_mounts": delegated_input_mounts,
                        "task_attachments": task_attachments,
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
        tracing::debug!(
            thread_id = self.ctx.thread_id,
            count = consumed,
            "marked subscription notifications consumed"
        );

        Ok(format!(
            "[Task subscription delivery]\nYou are subscribed to status changes on the tasks below. Each task's durable state is at the listed `/project_workspace/tasks/<task_key>` path (read TASK.md, JOURNAL.md, artifacts/ as needed). Decide whether to surface this to the user or take action — only mention items they would care about.\n\n{}",
            sections.join("\n")
        ))
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

    fn format_task_attachments(item: &models::ProjectTaskBoardItem) -> Vec<serde_json::Value> {
        item.typed_metadata()
            .attachments
            .into_iter()
            .map(|attachment| {
                json!({
                    "path": attachment.path,
                    "name": attachment.original_name,
                    "mime_type": attachment.mime_type,
                    "size_bytes": attachment.size_bytes,
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
        use super::hooks::LifecyclePhase;
        self.shell = self.shell.clone().with_cwd(self.default_shell_cwd());
        self.reconcile_agent_skills().await;
        self.run_hooks(LifecyclePhase::ExecutionStart, serde_json::Value::Null)
            .await;

        let keepalive = {
            let sandbox = self.sandbox.clone();
            let thread_id = self.ctx.thread_id;
            let label = sandbox.id().to_string();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5 * 60));
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    if let Err(e) = sandbox.touch().await {
                        tracing::warn!(
                            thread_id,
                            sandbox = %label,
                            error = ?e,
                            "sandbox keepalive: touch failed (will retry next tick)",
                        );
                    } else {
                        tracing::trace!(
                            thread_id,
                            sandbox = %label,
                            "sandbox keepalive: touched",
                        );
                    }
                }
            })
        };

        let result = self.repl_inner().await;

        keepalive.abort();

        self.cleanup_prompt_cache_on_finish().await;

        if result.is_err() {
            let _ = self.force_thread_idle_on_error_exit().await;
        }

        self.run_hooks(LifecyclePhase::ExecutionEnd, serde_json::Value::Null)
            .await;
        result
    }

    async fn force_thread_idle_on_error_exit(&self) -> Result<(), AppError> {
        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .with_status(AgentThreadStatus::Idle)
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await
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
        if let Err(exhausted) = self.budget.check() {
            self.run_hooks(
                super::hooks::LifecyclePhase::OnBudgetExhausted,
                serde_json::json!({ "reason": exhausted.reason() }),
            )
            .await;
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

        if self.consecutive_unproductive_turns >= MAX_UNPRODUCTIVE_TURNS {
            if self.can_abort_current_assignment_execution() {
                self.abort_current_assignment_execution(&AbortDirective {
                    reason: "Run stopped: repeated turns with no forward progress (note-spam, empty replies, or rejected calls). Coordinator should retry with a tighter brief or mark the task blocked.".to_string(),
                    outcome: AbortOutcome::Blocked,
                })
                .await?;
            } else {
                self.finish_without_summary().await?;
            }
            return Ok(false);
        }

        let prev_was_text_nudge = std::mem::take(&mut self.pending_text_nudge);
        let context_json = self.build_agent_loop_context_json().await?;
        let prompt_context: dto::json::AgentLoopPromptEnvelope =
            serde_json::from_value(context_json.clone()).map_err(|e| {
                AppError::Internal(format!("Failed to deserialize prompt context: {e}"))
            })?;
        let mut request = self.build_agent_loop_request(&prompt_context, &context_json, None)?;

        let available_tools = self.available_tools_for_mode().await;
        let active_board_item = self.active_board_item_prompt_item().await?;
        let mut native_tools: Vec<NativeToolDefinition> = available_tools
            .iter()
            .map(|t| self.build_native_tool_definition(t, active_board_item.as_ref()))
            .collect();

        let ask_user_enabled = !self
            .ctx
            .agent
            .disabled_internal_tools
            .iter()
            .any(|t| t == "ask_user");
        native_tools.push(note_tool());
        if ask_user_enabled {
            native_tools.push(ask_user_tool());
        }
        native_tools.push(complete_tool());
        if self.is_conversation_thread {
            native_tools.push(notify_user_tool());
        }
        if self.current_board_item_id().is_some() {
            native_tools.push(resolve_user_feedback_tool());
        }
        if self.should_offer_abort_task() {
            native_tools.push(abort_tool());
        }

        let turn_tool_names: Vec<String> = native_tools.iter().map(|t| t.name.clone()).collect();

        if self.complete_nudge_count >= 1 {
            request.forced_tool_names = Some(
                turn_tool_names
                    .iter()
                    .filter(|name| name.as_str() != "note" && name.as_str() != "ask_user")
                    .cloned()
                    .collect(),
            );
        }

        let llm = self.create_strong_llm().await?;
        let turn_provider = llm.provider_label().to_string();
        let turn_model = llm.model_name().to_string();

        let history_len = prompt_context.conversation_history_prefix.len();
        let total_messages = request.messages.len();
        let pre_history = total_messages.saturating_sub(history_len).min(1);
        let live_tail_count = total_messages.saturating_sub(pre_history + history_len);
        let cache_request = self.build_prompt_cache_request(live_tail_count).await;
        self.run_hooks(
            super::hooks::LifecyclePhase::BeforeLlm,
            serde_json::Value::Null,
        )
        .await;
        let mut output = llm
            .generate_tool_calls(request, native_tools, cache_request)
            .await?;
        self.run_hooks(
            super::hooks::LifecyclePhase::AfterLlm,
            serde_json::Value::Null,
        )
        .await;

        for call in output.calls.iter_mut() {
            let canonical = dto::json::tool_calls::canonical_tool_name(&call.tool_name);
            if canonical != call.tool_name {
                call.tool_name = canonical.to_string();
            }
        }

        {
            let mut produced = String::new();
            for call in &output.calls {
                let args: String = serde_json::to_string(&call.arguments)
                    .unwrap_or_default()
                    .chars()
                    .take(400)
                    .collect();
                produced.push_str(&format!("\n  • {} {}", call.tool_name, args));
            }
            if let Some(text) = output
                .content_text
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty())
            {
                let text: String = text.chars().take(1000).collect();
                produced.push_str(&format!("\n  text: {text}"));
            }
            if produced.is_empty() {
                produced.push_str(" <empty response>");
            }
            tracing::info!(
                target: "loop",
                "iter {} [{}/{}] finish={}{}",
                self.current_iteration,
                turn_provider,
                turn_model,
                output.finish_reason.as_deref().unwrap_or("-"),
                produced,
            );
        }

        let raw_output_snapshot = serde_json::to_string(&output).unwrap_or_default();
        self.budget.tick_llm();
        self.budget.tick_tools(
            output
                .calls
                .iter()
                .filter(|c| !is_meta_tool_name(&c.tool_name))
                .count(),
        );

        if let Some(usage) = output.usage_metadata.as_ref() {
            self.budget.tick_tokens(usage.total_token_count as u64);
        }

        self.record_llm_usage_for_compaction(output.usage_metadata.as_ref());
        if let Some(cache_state) = output.cache_state.as_ref() {
            self.write_prompt_cache_state(cache_state).await;
        }

        if let Some(leaked) = output
            .content_text
            .clone()
            .filter(|t| detect_leaked_tool_call(t))
        {
            let healed = tool_call_salvage::salvage(&leaked);
            let residual_len = healed
                .residual_text
                .as_deref()
                .map(|t| t.trim().chars().count())
                .unwrap_or(0);
            // Strip the markup from user-visible text regardless of length; only recover calls when it dominated.
            output.content_text = healed
                .residual_text
                .clone()
                .filter(|t| !detect_leaked_tool_call(t));
            if residual_len <= 240 {
                for mut call in healed.calls {
                    let canonical = dto::json::tool_calls::canonical_tool_name(&call.tool_name);
                    if canonical != call.tool_name {
                        call.tool_name = canonical.to_string();
                    }
                    output.calls.push(call);
                }
                if output.calls.is_empty() && output.content_text.is_none() {
                    self.note_unproductive_turn();
                    return Ok(true);
                }
            }
        }

        // Truncated turn (hit the output token limit): surface a steering signal so
        // the model shortens/splits next turn. The terminal-text path below also
        // refuses to treat truncated text as a final answer.
        let response_truncated = output
            .finish_reason
            .as_deref()
            .map(is_truncated_finish)
            .unwrap_or(false);
        if response_truncated {
            self.signal(core::RuntimeSignal::ResponseTruncated);
        }

        let note_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| c.tool_name == "note")
            .cloned()
            .collect();
        let ask_user_calls: Vec<_> = if ask_user_enabled {
            output
                .calls
                .iter()
                .filter(|c| c.tool_name == "ask_user")
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
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
            .filter(|c| c.tool_name == "terminate_loop")
            .cloned()
            .collect();
        let non_note_calls: Vec<_> = output
            .calls
            .iter()
            .filter(|c| {
                c.tool_name != "note"
                    && !(ask_user_enabled && c.tool_name == "ask_user")
                    && c.tool_name != "resolve_user_feedback"
                    && c.tool_name != "notify_user"
                    && c.tool_name != "terminate_loop"
            })
            .cloned()
            .collect();

        // Reset the complete-nudge only on a recognized non-note tool call —
        // an unknown/rejected name isn't progress and must not defeat the gate.
        if output
            .calls
            .iter()
            .any(|c| c.tool_name != "note" && turn_tool_names.contains(&c.tool_name))
        {
            self.complete_nudge_count = 0;
        }

        if output.calls.is_empty()
            && output
                .content_text
                .as_ref()
                .map(|t| t.trim().is_empty())
                .unwrap_or(true)
        {
            self.consecutive_empty_responses = self.consecutive_empty_responses.saturating_add(1);
        } else {
            self.consecutive_empty_responses = 0;
        }

        for call in &resolve_calls {
            self.handle_resolve_user_feedback_call(call).await?;
        }
        if !resolve_calls.is_empty() {
            self.reset_unproductive_turns();
        }

        // Persist notes before the ask_user / notify early returns, which exit the iteration.
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

        if let Some(call) = ask_user_calls.first() {
            self.reset_unproductive_turns();
            return self.handle_ask_user_call(call).await;
        }

        if let Some(call) = notify_calls.first() {
            self.reset_unproductive_turns();
            return self.handle_notify_user_call(call).await;
        }

        let note_only = !note_calls.is_empty()
            && non_note_calls.is_empty()
            && complete_calls.is_empty()
            && output
                .content_text
                .as_ref()
                .map(|t| t.trim().is_empty())
                .unwrap_or(true);

        if note_only {
            self.consecutive_note_count = self.consecutive_note_count.saturating_add(1);
            if self.consecutive_note_count >= 3 {
                self.signal(core::RuntimeSignal::NoteLoop {
                    count: self.consecutive_note_count,
                });
            }
            self.note_unproductive_turn();
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
                .map_err_internal("abort_task args malformed")?;
            self.abort_current_assignment_execution(&AbortDirective {
                outcome: args.outcome,
                reason: args.reason,
            })
            .await?;
            return Ok(false);
        }

        if let Some(complete_call) = complete_calls.first() {
            if non_note_calls.is_empty() {
                return self
                    .handle_complete_call(
                        complete_call,
                        output.content_text.as_deref(),
                        prev_was_text_nudge,
                    )
                    .await;
            }
            self.record_invalid_tool_call(
                "terminate_loop",
                &complete_call.arguments,
                "`terminate_loop` must be the only tool call in its response. The other tool calls in this turn ran; review their results, then call `terminate_loop` alone once the run is actually finished.",
            )
            .await?;
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
                if response_truncated {
                    // Cut-off text is not a complete answer — nudge, don't auto-complete.
                    self.note_unproductive_turn();
                    return Ok(true);
                }
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
            if self.consecutive_empty_responses >= 2 {
                if self.can_abort_current_assignment_execution() {
                    self.abort_current_assignment_execution(&AbortDirective {
                        reason: "Run stopped: model returned empty responses repeatedly."
                            .to_string(),
                        outcome: AbortOutcome::Blocked,
                    })
                    .await?;
                } else {
                    self.finish_without_summary().await?;
                }
                return Ok(false);
            }
            self.signal(core::RuntimeSignal::EmptyResponse);
            self.note_unproductive_turn();
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
        let mut tool_call_signatures: Vec<Option<String>> = Vec::new();
        let mut pending_shell_nudge: Option<String> = None;
        for call in non_note_calls
            .into_iter()
            .filter(|c| c.tool_name != "abort_task")
        {
            if call.tool_name == "execute_command" {
                let command = call
                    .arguments
                    .get("command")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if let shell_guard::ShellVerdict::Nudge(message) =
                    shell_guard::classify_shell_command(command)
                {
                    pending_shell_nudge.get_or_insert(message);
                }
            }
            let tool = match available_tools.iter().find(|t| t.name == call.tool_name) {
                Some(t) => t,
                None => {
                    let available_names = turn_tool_names.join(", ");
                    self.record_invalid_tool_call(
                        &call.tool_name,
                        &call.arguments,
                        &format!(
                            "Unknown tool '{}'. Not in this turn's allowed set. Available tools: [{}]. Pick one of these by exact name, or respond with text if none fit.",
                            call.tool_name, available_names
                        ),
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
                            dto::json::tool_calls::agent_facing_tool_name(&call.tool_name)
                        ),
                    )
                    .await?;
                    continue;
                }
            };
            match self.build_tool_call_request_from_native_call(tool, input_object) {
                Ok(req) => {
                    tool_requests.push(req);
                    tool_call_signatures.push(call.signature.clone());
                }
                Err(e) => {
                    self.record_invalid_tool_call(&call.tool_name, &call.arguments, &e.to_string())
                        .await?;
                }
            }
        }

        if tool_requests.is_empty() {
            self.note_unproductive_turn();
            return Ok(true);
        }

        if let Some(message) = pending_shell_nudge {
            self.consecutive_shell_nudge_count =
                self.consecutive_shell_nudge_count.saturating_add(1);
            if self.consecutive_shell_nudge_count >= SHELL_NUDGE_ESCALATE_AT {
                self.signal(core::RuntimeSignal::ShellDisciplineEscalated {
                    count: self.consecutive_shell_nudge_count,
                });
            } else {
                self.signal(core::RuntimeSignal::ShellDiscipline { message });
            }
        } else {
            self.consecutive_shell_nudge_count = 0;
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
            self.signal(core::RuntimeSignal::ToolCallLoop {
                count: self.repeated_tool_call_count + 1,
            });
            if self.repeated_tool_call_count >= 4 {
                self.note_unproductive_turn();
                return Ok(true);
            }
        }

        let batch_size = tool_requests.len();
        self.run_hooks(
            super::hooks::LifecyclePhase::BeforeTool,
            serde_json::Value::Null,
        )
        .await;
        let tool_calls_with_signatures: Vec<(ToolCallRequest, Option<String>)> =
            tool_requests.into_iter().zip(tool_call_signatures).collect();
        let outcome = self
            .execute_requested_actions(tool_calls_with_signatures, turn_provider, turn_model)
            .await?;
        self.reset_unproductive_turns();
        if batch_size >= LARGE_TOOL_BATCH {
            self.signal(core::RuntimeSignal::BatchBackpressure { batch_size });
        }
        self.run_hooks(
            super::hooks::LifecyclePhase::AfterTool,
            serde_json::Value::Null,
        )
        .await;
        self.finalize_action_execution_outcome(outcome).await
    }

    fn note_unproductive_turn(&mut self) {
        self.consecutive_unproductive_turns = self.consecutive_unproductive_turns.saturating_add(1);
    }

    fn reset_unproductive_turns(&mut self) {
        self.consecutive_unproductive_turns = 0;
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
        self.audit_rejected_call(tool_name, arguments, error).await;
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
        let safe_message =
            Self::sanitize_user_facing_message(&text, "Completed the requested work.");

        // Run ends only on an explicit terminate_loop (conversation included); a text-only turn is nudged, then auto-completes as a fallback. Service lanes get more rope.
        let max_complete_nudges = if self.is_service_mode_execution() {
            5
        } else {
            2
        };
        if self.complete_nudge_count < max_complete_nudges {
            self.complete_nudge_count += 1;
            self.pending_text_nudge = true;
            self.store_conversation(
                ConversationContent::Steer {
                    message: safe_message,
                    further_actions_required: true,
                    reasoning: "Text-only turn — run not completed yet.".to_string(),
                    attachments: None,
                },
                ConversationMessageType::Steer,
            )
            .await?;
            self.signal(core::RuntimeSignal::CompleteRequired);
            return Ok(true);
        }

        if let Some(reason) = self.completion_guard_error().await? {
            self.signal(core::RuntimeSignal::CompleteBlocked { reason });
            return Ok(true);
        }

        self.finalize_completion(
            meta_tools::CompletionHandoff::from_summary(safe_message.clone()),
            safe_message,
        )
        .await
    }

    pub(in crate::executor::agent_loop) async fn persist_task_handoff_summary(
        &self,
        handoff: &meta_tools::CompletionHandoff,
    ) {
        if self.is_conversation_thread {
            return;
        }
        let Some(board_item_id) = self.current_board_item_id() else {
            return;
        };

        let role_str = if self.is_coordinator_thread {
            "coordinator"
        } else if self.is_review_thread {
            "reviewer"
        } else {
            "executor"
        };

        let summary = handoff.summary.clone();
        let artifacts = handoff.artifacts.clone();
        let blockers = handoff.blockers.clone();
        let next_actions = handoff.next_actions.clone();

        let handoff_id = match self.ctx.app_state.sf.next_id() {
            Ok(id) => id as i64,
            Err(error) => {
                tracing::warn!(?error, "snowflake id allocation for task handoff failed");
                return;
            }
        };

        let mut cmd = commands::CreateTaskHandoffSummaryCommand::new(
            handoff_id,
            self.ctx.agent.deployment_id,
            board_item_id,
            self.ctx.thread_id,
            role_str,
            "completed",
            summary.clone(),
        )
        .with_execution_run_id(self.ctx.execution_run_id);
        if let Some(value) = artifacts.clone() {
            cmd = cmd.with_artifacts(value);
        }
        if let Some(value) = blockers.clone() {
            cmd = cmd.with_blockers(value);
        }
        if let Some(value) = next_actions.clone() {
            cmd = cmd.with_next_actions(value);
        }

        let inserted_handoff = match cmd
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
        {
            Ok(row) => row,
            Err(error) => {
                tracing::warn!(
                    thread_id = self.ctx.thread_id,
                    board_item_id,
                    ?error,
                    "failed to persist task handoff summary"
                );
                return;
            }
        };

        let recipient_thread_id = match self.resolve_handoff_recipient(board_item_id).await {
            Ok(Some(id)) if id != self.ctx.thread_id => id,
            Ok(_) => return,
            Err(error) => {
                tracing::warn!(
                    thread_id = self.ctx.thread_id,
                    board_item_id,
                    ?error,
                    "failed to resolve handoff recipient"
                );
                return;
            }
        };

        let recipient_conv_id = match self.ctx.app_state.sf.next_id() {
            Ok(id) => id as i64,
            Err(error) => {
                tracing::warn!(
                    ?error,
                    "snowflake id allocation for handoff conversation failed"
                );
                return;
            }
        };

        let content = ConversationContent::TaskHandoffReceived {
            source_thread_id: self.ctx.thread_id,
            board_item_id,
            source_role: role_str.to_string(),
            outcome: "completed".to_string(),
            summary,
            artifacts,
            blockers,
            next_actions,
            completed_at: inserted_handoff.created_at,
        };

        let mut conv_cmd = commands::CreateConversationCommand::new(
            recipient_conv_id,
            recipient_thread_id,
            content,
            models::ConversationMessageType::TaskHandoffReceived,
        )
        .with_board_item_id(board_item_id);
        conv_cmd = conv_cmd.with_metadata(serde_json::json!({
            "handoff_id": inserted_handoff.id.to_string(),
        }));

        if let Err(error) = conv_cmd
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
        {
            tracing::warn!(
                thread_id = self.ctx.thread_id,
                recipient_thread_id,
                board_item_id,
                ?error,
                "failed to write handoff conversation record on recipient thread"
            );
            return;
        }

        if let Err(error) = commands::mark_subscription_notifications_consumed_for_board_item(
            self.ctx.app_state.db_router.writer(),
            recipient_thread_id,
            board_item_id,
        )
        .await
        {
            tracing::warn!(
                recipient_thread_id,
                board_item_id,
                ?error,
                "failed to consume pending subscription notifications after handoff"
            );
        }
    }

    async fn resolve_handoff_recipient(&self, board_item_id: i64) -> Result<Option<i64>, AppError> {
        let Some(board_item) = queries::GetProjectTaskBoardItemByIdQuery::new(board_item_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?
        else {
            return Ok(None);
        };

        let is_delegated =
            board_item.metadata.get("kind").and_then(|v| v.as_str()) == Some("delegated_task");
        if is_delegated {
            if let Some(id) = board_item
                .metadata
                .get("delegated_by_thread_id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i64>().ok())
            {
                return Ok(Some(id));
            }
        }

        let Some(board) = queries::GetProjectTaskBoardByIdQuery::new(
            board_item.board_id,
            self.ctx.agent.deployment_id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?
        else {
            return Ok(None);
        };

        queries::GetProjectCoordinatorThreadIdQuery::new(board.project_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await
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
