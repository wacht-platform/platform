use super::core::AgentExecutor;

use crate::runtime::task_workspace::{
    prepare_task_workspace_layout_at_path, TaskWorkspaceBriefInput, TASK_WORKSPACE_DIR,
    TASK_WORKSPACE_JOURNAL_FILE, TASK_WORKSPACE_RUNBOOK_FILE, TASK_WORKSPACE_TASK_FILE,
};

use commands::UpdateAgentThreadStateCommand;
use common::error::AppError;
use models::{
    AgentThreadStatus, ConversationContent, ConversationMessageType, ProjectTaskBoardItemMetadata,
};
use std::path::Path;

#[derive(Debug, Clone)]
pub(crate) struct TaskWorkspaceContext {
    pub(crate) directory_path: String,
    pub(crate) task_file_path: String,
    pub(crate) journal_file_path: String,
    pub(crate) runbook_file_path: Option<String>,
}

impl AgentExecutor {
    pub(crate) async fn task_journal_tail_snippet(&self) -> Result<Option<String>, AppError> {
        let bytes = match self
            .filesystem
            .read_file_bytes(TASK_WORKSPACE_JOURNAL_FILE)
            .await
        {
            Ok(bytes) => bytes,
            Err(common::error::AppError::NotFound(_)) => return Ok(None),
            Err(err) => return Err(err),
        };

        let content = String::from_utf8_lossy(&bytes);
        let lines = content.lines().collect::<Vec<_>>();
        if lines.is_empty() {
            return Ok(None);
        }

        let take_lines = 60usize;
        let start = lines.len().saturating_sub(take_lines);
        let mut snippet = lines[start..].join("\n").trim().to_string();
        if snippet.is_empty() {
            return Ok(None);
        }

        if start > 0 {
            snippet = format!(
                "[Showing last {} of {} journal lines]\n{}",
                lines.len() - start,
                lines.len(),
                snippet
            );
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
            "needs_clarification" | "completed" | "blocked" | "waiting_for_children"
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

    pub(crate) async fn allow_complete_for_current_task_owner(&mut self) -> Result<bool, AppError> {
        let Some(board_item) = self.active_board_item_for_completion_guard().await? else {
            return Ok(true);
        };

        if !self.task_brief_is_ready().await? {
            self.store_conversation(
                ConversationContent::SystemDecision {
                    step: "terminal_stop_blocked_by_missing_task_brief".to_string(),
                    reasoning: format!(
                        "Terminal stop was blocked because `/task/TASK.md` is missing or too thin for task {}. Do not end this task stage yet. First create or refresh `/task/TASK.md` with the real operative task brief by using `write_file` or `edit_file`, then continue or conclude.",
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
                        "Completion was blocked because parent task {} still has incomplete child tasks: {}. Do not complete the parent yet. Use `update_project_task` to set the parent task status to `waiting_for_children`, then continue orchestration until all child tasks are completed.",
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
        let completion_allowed = if is_coordinator {
            assigned_thread_id
                .map(|thread_id| thread_id != self.ctx.thread_id)
                .unwrap_or(false)
                || Self::board_item_status_allows_coordinator_completion(&board_item.status)
        } else {
            self.can_abort_current_assignment_execution()
                || assigned_thread_id != Some(self.ctx.thread_id)
        };

        if completion_allowed {
            return Ok(true);
        }

        let reasoning = if is_coordinator {
            match assigned_thread_id {
                Some(thread_id) if thread_id == self.ctx.thread_id => format!(
                    "Completion was blocked because the coordinator still owns board item {} and its status is `{}`. Do not stop yet. Either use `assign_project_task` to move ownership to the next worker/reviewer thread, or use `update_project_task` to move the task to an allowed coordinator-held state such as `needs_clarification`, `blocked`, or `waiting_for_children` before concluding.",
                    board_item.task_key, board_item.status
                ),
                Some(thread_id) => format!(
                    "Completion was blocked because board item {} is assigned to thread {} but did not satisfy the coordinator completion rule. Do not stop yet. Inspect the board state and correct it by reassignment with `assign_project_task` or by updating the task status with `update_project_task` before concluding.",
                    board_item.task_key, thread_id
                ),
                None => format!(
                    "Completion was blocked because board item {} is unassigned and its status is `{}`. Do not stop yet. Either assign the next stage with `assign_project_task`, or update the task to a terminal/holding status such as `needs_clarification`, `blocked`, or `waiting_for_children` with `update_project_task` before concluding.",
                    board_item.task_key, board_item.status
                ),
            }
        } else {
            match assigned_thread_id {
                Some(thread_id) if thread_id == self.ctx.thread_id => format!(
                    "Completion was blocked because this non-coordinator thread still owns board item {} outside the runtime-managed assignment-execution completion path. Do not stop yet. If this is a normal assignment-execution run, let that path complete. Otherwise, continue the active work or hand control back before concluding.",
                    board_item.task_key
                ),
                Some(thread_id) => format!(
                    "Completion was blocked because board item {} is assigned to thread {} even though this thread no longer owns it. Do not stop yet. Refresh the board state and continue through the active assignee or coordinator path instead of concluding here.",
                    board_item.task_key, thread_id
                ),
                None => format!(
                    "Completion was blocked because board item {} is unassigned even though this thread no longer owns it. Do not stop yet. Return control to the coordinator flow so it can assign the next stage or update the task status explicitly.",
                    board_item.task_key
                ),
            }
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

    async fn task_brief_is_ready(&self) -> Result<bool, AppError> {
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

    pub(crate) async fn abort_current_assignment_execution(
        &mut self,
        directive: &dto::json::agent_executor::AbortDirective,
    ) -> Result<(), AppError> {
        if !self.can_abort_current_assignment_execution() {
            return Err(AppError::BadRequest(
                "abort is only valid for assignment execution threads".to_string(),
            ));
        }

        let note = directive.reason.trim();
        if note.is_empty() {
            return Err(AppError::BadRequest(
                "abort requires a non-empty reason".to_string(),
            ));
        }

        if matches!(
            directive.outcome,
            dto::json::agent_executor::AbortOutcome::Blocked
        ) {
            if let Some(board_item) = self.active_board_item_for_completion_guard().await? {
                self.update_project_task_board_item(
                    board_item.task_key,
                    Some("blocked".to_string()),
                    None,
                    ProjectTaskBoardItemMetadata {
                        kind: Some("project_task_aborted".to_string()),
                        tool_name: Some("abort".to_string()),
                        updated_at: Some(chrono::Utc::now().to_rfc3339()),
                    },
                    None,
                )
                .await?;
            }
        }

        self.cancel_active_task_graph_if_any().await?;

        let (assignment_status, result_status) = match directive.outcome {
            dto::json::agent_executor::AbortOutcome::Blocked => (
                models::project_task_board::assignment_status::BLOCKED.to_string(),
                Some(models::project_task_board::assignment_result_status::BLOCKED.to_string()),
            ),
            dto::json::agent_executor::AbortOutcome::ReturnToCoordinator => (
                models::project_task_board::assignment_status::CANCELLED.to_string(),
                Some(models::project_task_board::assignment_result_status::CANCELLED.to_string()),
            ),
        };

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
        state.assignment_outcome_override = Some(models::ThreadAssignmentOutcomeOverride {
            assignment_status,
            result_status,
            note: Some(note.to_string()),
        });

        UpdateAgentThreadStateCommand::new(self.ctx.thread_id, self.ctx.agent.deployment_id)
            .with_execution_state(state)
            .with_status(AgentThreadStatus::Idle)
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

    pub(crate) async fn ensure_task_workspace_for_key(
        &mut self,
        task_key: &str,
        title: &str,
        board_item_id: i64,
    ) -> Result<TaskWorkspaceContext, AppError> {
        let safe_task_key = Self::sanitize_task_path_segment(task_key);
        let persistent_task_path = self.filesystem.mount_task_workspace(&safe_task_key).await?;
        let is_recurring = if board_item_id > 0 {
            queries::GetProjectTaskScheduleByTemplateBoardItemIdQuery::new(board_item_id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?
                .is_some()
        } else {
            false
        };
        let prepared = prepare_task_workspace_layout_at_path(
            &persistent_task_path,
            &TaskWorkspaceBriefInput {
                task_key,
                title,
                is_recurring,
            },
        )
        .await?;
        self.sync_related_task_workspaces(&persistent_task_path, board_item_id)
            .await?;
        self.initialize_task_journal_start_hash(prepared.journal_hash.clone())
            .await?;

        Ok(TaskWorkspaceContext {
            directory_path: TASK_WORKSPACE_DIR.to_string(),
            task_file_path: TASK_WORKSPACE_TASK_FILE.to_string(),
            journal_file_path: TASK_WORKSPACE_JOURNAL_FILE.to_string(),
            runbook_file_path: is_recurring.then_some(TASK_WORKSPACE_RUNBOOK_FILE.to_string()),
        })
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

    pub(crate) async fn sync_related_task_workspaces(
        &self,
        active_task_path: &Path,
        board_item_id: i64,
    ) -> Result<(), AppError> {
        let related_root = active_task_path.join("related");
        let parent_mount = related_root.join("parent");
        let children_root = related_root.join("children");

        tokio::fs::create_dir_all(&related_root)
            .await
            .map_err(|err| {
                AppError::Internal(format!(
                    "Failed to prepare related task workspace root '{}': {}",
                    related_root.display(),
                    err
                ))
            })?;
        Self::remove_existing_mount_path(&parent_mount).await?;
        if tokio::fs::metadata(&children_root).await.is_ok() {
            tokio::fs::remove_dir_all(&children_root)
                .await
                .map_err(|err| {
                    AppError::Internal(format!(
                        "Failed to clear related child task mounts '{}': {}",
                        children_root.display(),
                        err
                    ))
                })?;
        }
        tokio::fs::create_dir_all(&children_root)
            .await
            .map_err(|err| {
                AppError::Internal(format!(
                    "Failed to prepare related child task mounts '{}': {}",
                    children_root.display(),
                    err
                ))
            })?;

        let relations = queries::ListProjectTaskBoardItemRelationsQuery::new(board_item_id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        for relation in relations {
            if relation.relation_type != models::project_task_board::relation_type::CHILD_OF {
                continue;
            }

            if relation.child_board_item_id == board_item_id {
                if let Some(parent_item) =
                    queries::GetProjectTaskBoardItemByIdQuery::new(relation.parent_board_item_id)
                        .execute_with_db(self.ctx.app_state.db_router.writer())
                        .await?
                {
                    self.mount_related_task_workspace(
                        active_task_path,
                        Path::new("related/parent"),
                        &parent_item,
                    )
                    .await?;
                }
                continue;
            }

            if relation.parent_board_item_id == board_item_id {
                if let Some(child_item) =
                    queries::GetProjectTaskBoardItemByIdQuery::new(relation.child_board_item_id)
                        .execute_with_db(self.ctx.app_state.db_router.writer())
                        .await?
                {
                    let child_mount_name = Self::sanitize_task_path_segment(&child_item.task_key);
                    self.mount_related_task_workspace(
                        active_task_path,
                        Path::new("related").join("children").join(child_mount_name),
                        &child_item,
                    )
                    .await?;
                }
            }
        }

        Ok(())
    }

    pub(crate) async fn mount_related_task_workspace<P: AsRef<Path>>(
        &self,
        active_task_path: &Path,
        relative_mount_path: P,
        related_item: &models::ProjectTaskBoardItem,
    ) -> Result<(), AppError> {
        let safe_task_key = Self::sanitize_task_path_segment(&related_item.task_key);
        let related_task_path = self.filesystem.persistent_task_path(&safe_task_key);
        prepare_task_workspace_layout_at_path(
            &related_task_path,
            &TaskWorkspaceBriefInput {
                task_key: &related_item.task_key,
                title: &related_item.title,
                is_recurring: false,
            },
        )
        .await?;

        let mount_path = active_task_path.join(relative_mount_path.as_ref());
        if let Some(parent) = mount_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|err| {
                AppError::Internal(format!(
                    "Failed to prepare related task mount parent '{}': {}",
                    parent.display(),
                    err
                ))
            })?;
        }

        Self::remove_existing_mount_path(&mount_path).await?;
        tokio::fs::symlink(&related_task_path, &mount_path)
            .await
            .map_err(|err| {
                AppError::Internal(format!(
                    "Failed to mount related task workspace '{}' -> '{}': {}",
                    mount_path.display(),
                    related_task_path.display(),
                    err
                ))
            })?;

        Ok(())
    }

    pub(crate) async fn remove_existing_mount_path(path: &Path) -> Result<(), AppError> {
        let metadata = match tokio::fs::symlink_metadata(path).await {
            Ok(metadata) => metadata,
            Err(_) => return Ok(()),
        };

        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            tokio::fs::remove_dir_all(path).await.map_err(|err| {
                AppError::Internal(format!(
                    "Failed to remove existing mount directory '{}': {}",
                    path.display(),
                    err
                ))
            })?;
        } else {
            tokio::fs::remove_file(path).await.map_err(|err| {
                AppError::Internal(format!(
                    "Failed to remove existing mount path '{}': {}",
                    path.display(),
                    err
                ))
            })?;
        }

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
