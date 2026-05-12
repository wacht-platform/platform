use super::core::AgentExecutor;

use crate::runtime::task_workspace::{
    compute_task_journal_hash, prepare_task_workspace, read_task_journal_tail,
    TaskWorkspaceBriefInput, TASK_WORKSPACE_DIR, TASK_WORKSPACE_JOURNAL_FILE,
    TASK_WORKSPACE_RUNBOOK_FILE, TASK_WORKSPACE_TASK_FILE,
};

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::{
    AgentThreadStatus, ConversationContent, ConversationMessageType, ProjectTaskBoardItemMetadata,
};

#[derive(Debug, Clone)]
pub(crate) struct TaskWorkspaceContext {
    pub(crate) directory_path: String,
    pub(crate) task_file_path: String,
    pub(crate) journal_file_path: String,
    pub(crate) runbook_file_path: Option<String>,
}

impl AgentExecutor {
    pub(crate) async fn task_journal_tail_snippet(&self) -> Result<Option<String>, AppError> {
        let Some(bytes) = read_task_journal_tail(&self.filesystem).await? else {
            return Ok(None);
        };

        let content = String::from_utf8_lossy(&bytes);
        let mut lines: Vec<&str> = content.lines().collect();
        let truncated_head = bytes.len() as u64 >= 16 * 1024;
        if truncated_head && !lines.is_empty() {
            lines.remove(0);
        }
        if lines.is_empty() {
            return Ok(None);
        }

        let take_lines = 60usize;
        let start = lines.len().saturating_sub(take_lines);
        let mut snippet = lines[start..].join("\n").trim().to_string();
        if snippet.is_empty() {
            return Ok(None);
        }

        if start > 0 || truncated_head {
            snippet = format!("[Showing tail of journal]\n{snippet}");
        }

        Ok(Some(snippet))
    }

    async fn active_board_item_for_completion_guard(
        &self,
    ) -> Result<Option<models::ProjectTaskBoardItem>, AppError> {
        let active_board_item_id = self
            .active_thread_event
            .as_ref()
            .and_then(|event| event.board_item_id)
            .or_else(|| {
                self.active_thread_event
                    .as_ref()
                    .and_then(|event| event.board_item_id)
            });

        match active_board_item_id {
            Some(item_id) => {
                queries::GetProjectTaskBoardItemByIdQuery::new(item_id)
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await
            }
            None => Ok(None),
        }
    }

    pub(crate) async fn active_board_item_prompt_item(
        &self,
    ) -> Result<Option<dto::json::ProjectTaskBoardPromptItem>, AppError> {
        Ok(self
            .active_board_item_for_completion_guard()
            .await?
            .map(|item| {
                self.project_task_board_items
                    .iter()
                    .find(|candidate| candidate.task_key == item.task_key)
                    .cloned()
                    .unwrap_or_else(|| Self::project_task_board_item_to_prompt_item(&item))
            }))
    }

    fn board_item_status_allows_coordinator_completion(status: &str) -> bool {
        matches!(
            status,
            "needs_clarification" | "completed" | "cancelled" | "blocked" | "waiting_for_children"
        )
    }

    async fn incomplete_child_tasks_for_board_item(
        &self,
        board_item: &models::ProjectTaskBoardItem,
    ) -> Result<Vec<models::ProjectTaskBoardItem>, AppError> {
        let relations = queries::ListProjectTaskBoardItemRelationsQuery::new(board_item.id)
            .execute_with_db(
                self.ctx
                    .app_state
                    .db_router
                    .reader(common::ReadConsistency::Strong),
            )
            .await?;

        let mut children = Vec::new();
        for relation in relations {
            if relation.relation_type != models::project_task_board::relation_type::CHILD_OF
                || relation.parent_board_item_id != board_item.id
            {
                continue;
            }

            let Some(child) =
                queries::GetProjectTaskBoardItemByIdQuery::new(relation.child_board_item_id)
                    .execute_with_db(
                        self.ctx
                            .app_state
                            .db_router
                            .reader(common::ReadConsistency::Strong),
                    )
                    .await?
            else {
                continue;
            };

            if child.status != "completed" {
                children.push(child);
            }
        }

        Ok(children)
    }

    pub(crate) fn is_service_mode_execution(&self) -> bool {
        self.active_thread_event
            .as_ref()
            .map(|event| event.event_type == models::thread_event::event_type::ASSIGNMENT_EXECUTION)
            .unwrap_or(false)
    }

    pub(crate) async fn service_mode_journal_was_updated(&self) -> Result<bool, AppError> {
        let Some(start_hash) = self.task_journal_start_hash.as_ref() else {
            return Ok(true);
        };
        if self
            .active_board_item_for_completion_guard()
            .await?
            .is_none()
        {
            return Ok(true);
        }
        let current_hash = compute_task_journal_hash(&self.filesystem).await?;
        Ok(&current_hash != start_hash)
    }

    pub(crate) async fn allow_complete_for_current_task_owner(&mut self) -> Result<bool, AppError> {
        let Some(board_item) = self.active_board_item_for_completion_guard().await? else {
            return Ok(true);
        };

        if self.effective_is_coordinator_thread() && !self.task_brief_is_ready().await? {
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "terminal_stop_blocked_by_missing_task_brief".to_string(),
                    reasoning: format!(
                        "I tried to terminate this turn, but the runtime stopped me. `/task/TASK.md` for task {} is missing or too thin — there's no real operative brief on disk yet, so any executor I route to would be working blind. I should not end the turn here. I'll write a concrete brief to `/task/TASK.md` (title, context, numbered acceptance criteria, scope boundaries) using `write_file`, then continue routing or conclude.",
                        board_item.task_key
                    ),
                    confidence: 1.0,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;

            return Ok(false);
        }

        let incomplete_children = if self.effective_is_coordinator_thread() {
            self.incomplete_child_tasks_for_board_item(&board_item)
                .await?
        } else {
            Vec::new()
        };
        if board_item.status == "completed" && !incomplete_children.is_empty() {
            let child_task_keys = incomplete_children
                .iter()
                .map(|child| child.task_key.clone())
                .collect::<Vec<_>>();

            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "complete_blocked_by_incomplete_child_tasks".to_string(),
                    reasoning: format!(
                        "I tried to mark parent task {} `completed`, but the runtime stopped me. These child tasks are still open: {}. Marking the parent done now would orphan unfinished work. I should not complete the parent yet. I'll call `update_project_task` to move the parent to `waiting_for_children` and let orchestration finish the children first; once they're all `completed` the parent will be ready to close.",
                        board_item.task_key,
                        child_task_keys.join(", ")
                    ),
                    confidence: 1.0,
                },
                ConversationMessageType::SystemDecision,
            )
            .await?;

            return Ok(false);
        }

        let assigned_thread_id = board_item.assigned_thread_id;
        let is_coordinator = self.effective_is_coordinator_thread();
        let active_assignments =
            queries::ListProjectTaskBoardItemAssignmentsQuery::new(board_item.id)
                .execute_with_db(
                    self.ctx
                        .app_state
                        .db_router
                        .reader(common::ReadConsistency::Strong),
                )
                .await?
                .into_iter()
                .filter(|a| {
                    matches!(
                        a.status.as_str(),
                        "pending" | "available" | "claimed" | "in_progress",
                    )
                })
                .collect::<Vec<_>>();
        let has_active_assignment_elsewhere = active_assignments
            .iter()
            .any(|a| a.thread_id != self.ctx.thread_id);
        let completion_allowed = if is_coordinator {
            has_active_assignment_elsewhere
                || Self::board_item_status_allows_coordinator_completion(&board_item.status)
        } else {
            self.can_abort_current_assignment_execution()
                || assigned_thread_id != Some(self.ctx.thread_id)
        };

        if completion_allowed {
            return Ok(true);
        }

        let reasoning = if is_coordinator {
            format!(
                "I tried to terminate this turn, but the runtime stopped me. Task {} is in status `{}` and no active assignment is currently routed away from me — so terminating now would leave the task stalled with the coordinator (me) still owning it. I should not just stop. My options: (a) call `assign_project_task` to route the next stage to an executor/reviewer thread, or (b) call `update_project_task` to move the task to a holding state I'm allowed to terminate at — `needs_clarification`, `blocked`, `waiting_for_children`, or `completed`. I'll pick the one that matches what just happened in this conversation, then end the turn.",
                board_item.task_key, board_item.status
            )
        } else {
            format!(
                "I tried to terminate this turn, but the runtime stopped me. I'm not the coordinator and task {} still belongs to me — terminating without finishing the assignment would leave the task hanging. I should not just stop. If I'm in an assignment-execution run, I need to let it finish through the normal completion path (it will produce a `result_summary` and the coordinator will pick it up). Otherwise I should continue the work or hand control back instead of ending here.",
                board_item.task_key
            )
        };

        self.store_conversation(
            ConversationContent::SystemDecision {
                step: "complete_blocked_by_task_ownership".to_string(),
                reasoning,
                confidence: 1.0,
            },
            ConversationMessageType::SystemDecision,
        )
        .await?;

        Ok(false)
    }

    pub(crate) async fn task_brief_is_ready(&self) -> Result<bool, AppError> {
        let bytes = match self
            .filesystem
            .read_file_bytes(TASK_WORKSPACE_TASK_FILE)
            .await
        {
            Ok(bytes) => bytes,
            Err(common::error::AppError::NotFound(_)) => return Ok(false),
            Err(err) => return Err(err),
        };

        let content = String::from_utf8_lossy(&bytes);
        let trimmed = content.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }

        let line_count = trimmed.lines().count();
        let char_count = trimmed.chars().count();

        Ok(line_count >= 5 && char_count >= 120)
    }

    async fn cancel_active_task_graph_if_any(&mut self) -> Result<(), AppError> {
        let maybe_graph = queries::GetLatestThreadTaskGraphQuery::new(
            self.ctx.agent.deployment_id,
            self.ctx.thread_id,
        )
        .with_board_item_id(self.current_board_item_id())
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;

        let Some(graph) = maybe_graph else {
            return Ok(());
        };

        if matches!(
            graph.status.as_str(),
            models::thread_task_graph::status::GRAPH_COMPLETED
                | models::thread_task_graph::status::GRAPH_FAILED
                | models::thread_task_graph::status::GRAPH_CANCELLED
        ) {
            self.invalidate_task_graph_snapshot();
            return Ok(());
        }

        commands::CancelThreadTaskGraphCommand { graph_id: graph.id }
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        self.invalidate_task_graph_snapshot();
        Ok(())
    }

    #[tracing::instrument(
        name = "assignment.abort",
        skip(self, directive),
        fields(
            thread_id = self.ctx.thread_id,
            board_item_id = ?self.current_board_item_id(),
            execution_run_id = self.ctx.execution_run_id,
            outcome = ?directive.outcome,
        )
    )]
    pub(crate) async fn abort_current_assignment_execution(
        &mut self,
        directive: &dto::json::agent_executor::AbortDirective,
    ) -> Result<(), AppError> {
        let note = directive.reason.trim();
        if note.is_empty() {
            return Err(AppError::BadRequest(
                "abort requires a non-empty reason".to_string(),
            ));
        }

        let is_service_run = self.can_abort_current_assignment_execution();
        let is_blocked_outcome = matches!(
            directive.outcome,
            dto::json::agent_executor::AbortOutcome::Blocked
        );

        // If we're running against a board item (coordinator or service run), and the
        // agent signalled a real block, transition the board item to `blocked`. This
        // keeps routing consistent across both modes.
        if is_blocked_outcome {
            if let Some(board_item) = self.active_board_item_for_completion_guard().await? {
                self.update_project_task_board_item(
                    board_item.task_key,
                    Some("blocked".to_string()),
                    ProjectTaskBoardItemMetadata {
                        kind: Some("project_task_aborted".to_string()),
                        tool_name: Some("abort".to_string()),
                        updated_at: Some(chrono::Utc::now().to_rfc3339()),
                        ..Default::default()
                    },
                    None,
                )
                .await?;
            }
        }

        self.cancel_active_task_graph_if_any().await?;

        self.store_conversation(
            ConversationContent::SystemDecision {
                step: "abort".to_string(),
                reasoning: note.to_string(),
                confidence: 1.0,
            },
            ConversationMessageType::SystemDecision,
        )
        .await?;

        let mut state = self.build_execution_state_snapshot(None);

        // Only service runs have an in-flight assignment to transition on abort.
        // Coordinator / conversation runs just idle the thread.
        if is_service_run {
            let (assignment_status, result_status) = match directive.outcome {
                dto::json::agent_executor::AbortOutcome::Blocked => (
                    models::project_task_board::assignment_status::BLOCKED.to_string(),
                    Some(models::project_task_board::assignment_result_status::BLOCKED.to_string()),
                ),
                dto::json::agent_executor::AbortOutcome::ReturnToCoordinator => (
                    models::project_task_board::assignment_status::CANCELLED.to_string(),
                    Some(
                        models::project_task_board::assignment_result_status::CANCELLED.to_string(),
                    ),
                ),
            };
            state.assignment_outcome_override = Some(models::ThreadAssignmentOutcomeOverride {
                assignment_status,
                result_status,
                note: Some(note.to_string()),
            });
        }

        self.apply_thread_status(
            UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
                .with_execution_state(state),
            AgentThreadStatus::Idle,
        )
        .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
        .await?;

        Ok(())
    }

    pub(crate) async fn load_board_item_for_thread_event(
        &self,
        thread_event: &models::ThreadEvent,
        fallback_board_item_id: Option<i64>,
    ) -> Result<Option<models::ProjectTaskBoardItem>, AppError> {
        let board_item_id = fallback_board_item_id
            .or(thread_event.board_item_id)
            .filter(|id| *id > 0);

        match board_item_id {
            Some(board_item_id) => {
                queries::GetProjectTaskBoardItemByIdQuery::new(board_item_id)
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await
            }
            None => Ok(None),
        }
    }

    pub(crate) fn fallback_task_key(
        thread_event: &models::ThreadEvent,
        board_item_id: Option<i64>,
    ) -> String {
        board_item_id
            .filter(|id| *id > 0)
            .map(|id| format!("task-{id}"))
            .unwrap_or_else(|| format!("thread-event-{}", thread_event.id))
    }

    pub(crate) async fn prepare_task_workspace_for_key(
        &self,
        task_key: &str,
        title: &str,
        is_recurring: bool,
    ) -> Result<(TaskWorkspaceContext, String), AppError> {
        let safe_task_key = Self::sanitize_task_path_segment(task_key);
        let prepared = prepare_task_workspace(
            &self.filesystem,
            &TaskWorkspaceBriefInput {
                task_key: &safe_task_key,
                title,
                is_recurring,
            },
        )
        .await?;
        Ok((
            TaskWorkspaceContext {
                directory_path: TASK_WORKSPACE_DIR.to_string(),
                task_file_path: TASK_WORKSPACE_TASK_FILE.to_string(),
                journal_file_path: TASK_WORKSPACE_JOURNAL_FILE.to_string(),
                runbook_file_path: is_recurring.then_some(TASK_WORKSPACE_RUNBOOK_FILE.to_string()),
            },
            prepared.journal_hash,
        ))
    }

    pub(crate) async fn initialize_task_journal_start_hash(
        &mut self,
        hash: String,
    ) -> Result<(), AppError> {
        self.task_journal_start_hash = Some(hash);

        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(self.build_execution_state_snapshot(None))
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;

        self.ctx.invalidate_cache();
        Ok(())
    }

    pub(crate) fn sanitize_task_path_segment(raw: &str) -> String {
        let sanitized: String = raw
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        if sanitized.is_empty() {
            "task".to_string()
        } else {
            sanitized
        }
    }
}
