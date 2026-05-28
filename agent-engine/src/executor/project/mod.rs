mod assignments;
pub(crate) mod prompt_items;
pub(crate) mod status_machine;
mod task_commands;
mod task_graph;
pub(crate) use super::core;

use super::core::AgentExecutor;
use crate::runtime::thread_execution_context::ThreadExecutionContext;
use commands::{
    AttachProjectTaskBoardItemScheduleCommand, CreateProjectTaskBoardItemCommand,
    CreateProjectTaskBoardItemRelationCommand, CreateProjectTaskScheduleCommand,
    EnsureProjectTaskBoardCommand, ReconcileProjectTaskBoardItemCommand,
    UpdateProjectTaskBoardItemCommand, UpdateProjectTaskScheduleCommand,
};
use common::error::AppError;
use dto::json::ProjectTaskBoardPromptItem;
use models::{ProjectTaskBoardItem, ProjectTaskBoardItemMetadata, ScheduleTemplatePayload};
use queries::{
    GetProjectTaskBoardByProjectIdQuery, GetProjectTaskBoardItemByTaskKeyQuery,
    GetProjectTaskScheduleByTaskKeyQuery, ListProjectTaskBoardItemsQuery,
    ListProjectTaskBoardRelationsQuery,
};
use std::collections::HashMap;

pub(crate) async fn lookup_or_create_project_task_board_id(
    ctx: &ThreadExecutionContext,
) -> Result<i64, AppError> {
    let thread = ctx.get_thread().await?;
    let existing =
        GetProjectTaskBoardByProjectIdQuery::new(thread.project_id, thread.deployment_id)
            .execute_with_db(ctx.app_state.db_router.writer())
            .await?;

    let board = match existing {
        Some(board) => board,
        None => {
            EnsureProjectTaskBoardCommand::new(
                ctx.app_state.sf.next_id()? as i64,
                thread.deployment_id,
                thread.actor_id,
                thread.project_id,
                format!("Project {} Task Board", thread.project_id),
                "active".to_string(),
            )
            .execute_with_db(ctx.app_state.db_router.writer())
            .await?
        }
    };

    Ok(board.id)
}

pub(crate) async fn load_project_task_board_state(
    ctx: &ThreadExecutionContext,
) -> Result<(i64, Vec<ProjectTaskBoardPromptItem>), AppError> {
    let board_id = lookup_or_create_project_task_board_id(ctx).await?;
    let items_query = ListProjectTaskBoardItemsQuery::new(board_id).include_agent_owned();
    let relations_query = ListProjectTaskBoardRelationsQuery::new(board_id);
    let (rows, relations) = tokio::try_join!(
        items_query.execute_with_schedules(ctx.app_state.db_router.writer()),
        relations_query.execute_with_db(ctx.app_state.db_router.writer()),
    )?;

    let task_key_by_item_id: HashMap<i64, String> = rows
        .iter()
        .map(|row| (row.item.id, row.item.task_key.clone()))
        .collect();
    let mut parent_task_key_by_child_id: HashMap<i64, String> = HashMap::new();
    let mut child_task_keys_by_parent_id: HashMap<i64, Vec<String>> = HashMap::new();

    for relation in relations {
        if relation.relation_type != models::project_task_board::relation_type::CHILD_OF {
            continue;
        }
        let Some(parent_task_key) = task_key_by_item_id
            .get(&relation.parent_board_item_id)
            .cloned()
        else {
            continue;
        };
        let Some(child_task_key) = task_key_by_item_id
            .get(&relation.child_board_item_id)
            .cloned()
        else {
            continue;
        };
        parent_task_key_by_child_id.insert(relation.child_board_item_id, parent_task_key);
        child_task_keys_by_parent_id
            .entry(relation.parent_board_item_id)
            .or_default()
            .push(child_task_key);
    }

    let prompt_items = rows
        .iter()
        .map(|row| {
            AgentExecutor::project_task_board_item_to_prompt_item_with_relations(
                &row.item,
                parent_task_key_by_child_id.get(&row.item.id).cloned(),
                child_task_keys_by_parent_id
                    .remove(&row.item.id)
                    .unwrap_or_default(),
                row.schedule.as_ref(),
            )
        })
        .collect();

    Ok((board_id, prompt_items))
}

impl AgentExecutor {
    pub(super) async fn ensure_project_task_board_id(&mut self) -> Result<i64, AppError> {
        if let Some(board_id) = self.project_task_board_id {
            return Ok(board_id);
        }
        let board_id = lookup_or_create_project_task_board_id(&self.ctx).await?;
        self.project_task_board_id = Some(board_id);
        Ok(board_id)
    }

    pub(super) async fn refresh_project_task_board_items(&mut self) -> Result<(), AppError> {
        let (board_id, items) = load_project_task_board_state(&self.ctx).await?;
        self.project_task_board_id = Some(board_id);
        self.project_task_board_items = items;
        Ok(())
    }

    pub(super) async fn create_project_task_board_item(
        &mut self,
        board_item_id: i64,
        title: String,
        description: Option<String>,
        status: String,
        parent_task_key: Option<String>,
        metadata: ProjectTaskBoardItemMetadata,
        schedule: Option<(String, chrono::DateTime<chrono::Utc>, Option<i64>)>,
        subscribe_for_thread_id: Option<i64>,
    ) -> Result<ProjectTaskBoardItem, AppError> {
        let board_id = self.ensure_project_task_board_id().await?;
        let mut tx = self.ctx.app_state.db_router.writer().begin().await?;

        let parent_board_item = match parent_task_key.as_ref() {
            Some(task_key) => Some(
                GetProjectTaskBoardItemByTaskKeyQuery::new(board_id, task_key.as_str())
                    .execute_with_db(&mut *tx)
                    .await?
                    .ok_or_else(|| {
                        AppError::BadRequest(format!(
                            "Parent task '{}' was not found in the current board",
                            task_key
                        ))
                    })?,
            ),
            None => None,
        };

        let mut item = CreateProjectTaskBoardItemCommand {
            id: board_item_id,
            board_id,
            task_key: format!("TASK-{board_item_id}"),
            title,
            description,
            status,
            assigned_thread_id: None,
            metadata: serde_json::to_value(metadata)?,
            mounts: serde_json::json!([]),
            exclusive_owner_agent_id: None,
        }
        .execute_with_db(&mut *tx)
        .await?;

        if let Some(parent_board_item) = parent_board_item {
            CreateProjectTaskBoardItemRelationCommand {
                id: self.ctx.app_state.sf.next_id()? as i64,
                board_id,
                parent_board_item_id: parent_board_item.id,
                child_board_item_id: item.id,
                relation_type: models::project_task_board::relation_type::CHILD_OF.to_string(),
                metadata: serde_json::json!({
                    "kind": "project_task_child_link",
                    "source": "create_project_task"
                }),
            }
            .execute_with_tx(&mut tx)
            .await?;

            if !matches!(
                parent_board_item.status.as_str(),
                "waiting_for_children"
                    | "completed"
                    | "failed"
                    | "blocked"
                    | "cancelled"
                    | "rejected"
                    | "needs_clarification"
                    | "needs_replan"
            ) {
                UpdateProjectTaskBoardItemCommand {
                    deployment_id: self.ctx.agent.deployment_id,
                    board_id,
                    task_key: parent_board_item.task_key.clone(),
                    status: Some("waiting_for_children".to_string()),
                    metadata: parent_board_item.metadata.clone(),
                }
                .execute_with_db(&mut *tx)
                .await?;
            }
        }

        if let Some((schedule_kind, next_run_at, interval_seconds)) = schedule {
            let project_id = self.ctx.get_thread().await?.project_id;
            let schedule = CreateProjectTaskScheduleCommand {
                id: self.ctx.app_state.sf.next_id()? as i64,
                board_id,
                project_id,
                task_key: item.task_key.clone(),
                template_payload: build_schedule_template_payload(&item),
                schedule_kind,
                interval_seconds,
                next_run_at,
                overlap_policy: None,
                mounts: None,
            }
            .execute_with_db(&mut *tx)
            .await?;
            item = AttachProjectTaskBoardItemScheduleCommand {
                board_id,
                task_key: item.task_key.clone(),
                schedule_id: schedule.id,
                mounts: schedule.mounts,
            }
            .execute_with_db(&mut *tx)
            .await?;
        }

        if let Some(thread_id) = subscribe_for_thread_id {
            commands::UpsertAgentThreadTaskSubscriptionCommand {
                deployment_id: self.ctx.agent.deployment_id,
                thread_id,
                board_item_id: item.id,
                event_kinds: models::TaskSubscriptionEventKind::defaults(),
            }
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        ReconcileProjectTaskBoardItemCommand::new(item.id)
            .with_note("Task created; scheduler evaluated initial routing".to_string())
            .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
            .await?;
        self.refresh_project_task_board_items().await?;
        Ok(item)
    }

    pub(super) async fn update_project_task_board_item(
        &mut self,
        task_key: String,
        status: Option<String>,
        metadata: ProjectTaskBoardItemMetadata,
        schedule: Option<(String, chrono::DateTime<chrono::Utc>, Option<i64>)>,
    ) -> Result<ProjectTaskBoardItem, AppError> {
        let board_id = self.ensure_project_task_board_id().await?;
        let mut item = UpdateProjectTaskBoardItemCommand {
            deployment_id: self.ctx.agent.deployment_id,
            board_id,
            task_key,
            status,
            metadata: serde_json::to_value(metadata)?,
        }
        .execute_with_deps(&common::deps::from_app(&self.ctx.app_state).db().nats().id())
        .await?;

        if let Some((schedule_kind, next_run_at, interval_seconds)) = schedule {
            let existing_schedule =
                GetProjectTaskScheduleByTaskKeyQuery::new(board_id, item.task_key.clone())
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await?;

            if let Some(existing_schedule) = existing_schedule {
                let schedule = UpdateProjectTaskScheduleCommand::new(existing_schedule.id)
                    .with_status(models::project_task_schedule::status::ACTIVE.to_string())
                    .with_interval_seconds(interval_seconds)
                    .with_next_run_at(next_run_at)
                    .with_template_payload(build_schedule_template_payload(&item))
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await?;
                item = AttachProjectTaskBoardItemScheduleCommand {
                    board_id,
                    task_key: item.task_key.clone(),
                    schedule_id: schedule.id,
                    mounts: schedule.mounts,
                }
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?;
            } else {
                let project_id = self.ctx.get_thread().await?.project_id;
                let schedule = CreateProjectTaskScheduleCommand {
                    id: self.ctx.app_state.sf.next_id()? as i64,
                    board_id,
                    project_id,
                    task_key: item.task_key.clone(),
                    template_payload: build_schedule_template_payload(&item),
                    schedule_kind,
                    interval_seconds,
                    next_run_at,
                    overlap_policy: None,
                    mounts: None,
                }
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?;
                item = AttachProjectTaskBoardItemScheduleCommand {
                    board_id,
                    task_key: item.task_key.clone(),
                    schedule_id: schedule.id,
                    mounts: schedule.mounts,
                }
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?;
            }
        }

        self.refresh_project_task_board_items().await?;
        Ok(item)
    }
}

fn build_schedule_template_payload(item: &ProjectTaskBoardItem) -> ScheduleTemplatePayload {
    ScheduleTemplatePayload {
        title: item.title.clone(),
        description: item.description.clone(),
        metadata: item.typed_metadata(),
    }
}
