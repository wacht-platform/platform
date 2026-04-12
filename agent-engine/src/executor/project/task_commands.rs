use super::core::AgentExecutor;

use common::error::AppError;
use dto::json::agent_executor::{
    AssignProjectTaskParams, CreateProjectTaskParams, UpdateProjectTaskParams,
};
use dto::json::ProjectTaskScheduleParams;
use models::ProjectTaskBoardItemMetadata;
use serde_json::Value;

fn create_project_task_resolved_status(params: &CreateProjectTaskParams) -> String {
    params
        .status
        .clone()
        .unwrap_or_else(|| "pending".to_string())
}

fn create_project_task_resolved_priority(params: &CreateProjectTaskParams) -> String {
    match params.priority.as_deref() {
        Some(models::project_task_board::task_priority::URGENT) => {
            models::project_task_board::task_priority::URGENT.to_string()
        }
        Some(models::project_task_board::task_priority::HIGH) => {
            models::project_task_board::task_priority::HIGH.to_string()
        }
        Some(models::project_task_board::task_priority::LOW) => {
            models::project_task_board::task_priority::LOW.to_string()
        }
        _ => models::project_task_board::task_priority::NEUTRAL.to_string(),
    }
}

fn create_project_task_resolved_parent_task_key(
    params: &CreateProjectTaskParams,
) -> Option<String> {
    params
        .parent_task_key
        .as_ref()
        .map(|task_key| task_key.trim())
        .filter(|task_key| !task_key.is_empty())
        .map(|task_key| task_key.to_string())
}

fn create_project_task_metadata() -> ProjectTaskBoardItemMetadata {
    ProjectTaskBoardItemMetadata {
        kind: Some("project_task_created".to_string()),
        tool_name: Some("create_project_task".to_string()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    }
}

fn update_project_task_has_meaningful_mutation(params: &UpdateProjectTaskParams) -> bool {
    params.status.is_some() || params.priority.is_some() || params.schedule.is_some()
}

fn validate_schedule_params(
    schedule: &ProjectTaskScheduleParams,
) -> Result<(String, chrono::DateTime<chrono::Utc>, Option<i64>), AppError> {
    let kind = schedule.kind.trim().to_string();
    let next_run_at = chrono::DateTime::parse_from_rfc3339(schedule.next_run_at.trim())
        .map_err(|err| {
            AppError::BadRequest(format!(
                "Invalid schedule.next_run_at '{}': {}",
                schedule.next_run_at, err
            ))
        })?
        .with_timezone(&chrono::Utc);
    match kind.as_str() {
        "once" => {
            if schedule.interval_seconds.is_some() {
                return Err(AppError::BadRequest(
                    "Schedule kind 'once' must not set interval_seconds".to_string(),
                ));
            }
            Ok((kind, next_run_at, None))
        }
        "interval" => {
            let interval_seconds = schedule.interval_seconds.unwrap_or(0);
            if interval_seconds <= 0 {
                return Err(AppError::BadRequest(
                    "Schedule kind 'interval' requires interval_seconds > 0".to_string(),
                ));
            }
            Ok((kind, next_run_at, Some(interval_seconds)))
        }
        _ => Err(AppError::BadRequest(format!(
            "Unsupported schedule kind '{}'",
            schedule.kind
        ))),
    }
}

fn normalize_schedule_params(
    schedule: &ProjectTaskScheduleParams,
) -> ProjectTaskScheduleParams {
    let kind = schedule.kind.trim().to_string();
    let next_run_at = schedule.next_run_at.trim().to_string();
    let interval_seconds = match kind.as_str() {
        "once" => None,
        _ => schedule.interval_seconds,
    };

    ProjectTaskScheduleParams {
        kind,
        next_run_at,
        interval_seconds,
    }
}

fn update_project_task_resolved_priority(params: &UpdateProjectTaskParams) -> Option<String> {
    match params.priority.as_deref() {
        Some(models::project_task_board::task_priority::URGENT) => {
            Some(models::project_task_board::task_priority::URGENT.to_string())
        }
        Some(models::project_task_board::task_priority::HIGH) => {
            Some(models::project_task_board::task_priority::HIGH.to_string())
        }
        Some(models::project_task_board::task_priority::LOW) => {
            Some(models::project_task_board::task_priority::LOW.to_string())
        }
        Some(_) => Some(models::project_task_board::task_priority::NEUTRAL.to_string()),
        None => None,
    }
}

fn update_project_task_metadata() -> ProjectTaskBoardItemMetadata {
    ProjectTaskBoardItemMetadata {
        kind: Some("project_task_updated".to_string()),
        tool_name: Some("update_project_task".to_string()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    }
}

impl AgentExecutor {
    pub(crate) async fn handle_create_project_task(
        &mut self,
        params: CreateProjectTaskParams,
    ) -> Result<Value, AppError> {
        if !self.can_create_project_task_in_current_mode() {
            return Err(AppError::BadRequest(
                "create_project_task is available only to the coordinator thread or a user-facing conversation thread".to_string(),
            ));
        }

        let title = params.title.trim().to_string();
        if title.is_empty() {
            return Err(AppError::BadRequest(
                "create_project_task requires a non-empty title".to_string(),
            ));
        }

        let board_item_id = self.ctx.app_state.sf.next_id()? as i64;
        let parent_task_key = create_project_task_resolved_parent_task_key(&params);
        let description = params.description.clone();
        let status = create_project_task_resolved_status(&params);
        let priority = create_project_task_resolved_priority(&params);
        let metadata = create_project_task_metadata();
        let schedule = params
            .schedule
            .as_ref()
            .map(normalize_schedule_params)
            .as_ref()
            .map(validate_schedule_params)
            .transpose()?;

        let board_item = self
            .create_project_task_board_item(
                board_item_id,
                title,
                description,
                status,
                priority,
                parent_task_key.clone(),
                metadata,
                schedule,
            )
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "create_project_task",
            "created_task_key": board_item.task_key,
            "task_key": board_item.task_key,
            "parent_task_key": parent_task_key,
            "created": true,
            "routed_to_coordinator": true,
            "created_board_item_id": board_item.id.to_string(),
            "board_item_id": board_item.id.to_string(),
        }))
    }

    pub(crate) async fn handle_update_project_task(
        &mut self,
        params: UpdateProjectTaskParams,
    ) -> Result<Value, AppError> {
        if !self.can_write_project_task_board_in_current_mode() {
            return Err(AppError::BadRequest(
                "update_project_task is available only to the coordinator thread or while handling an assignment event".to_string(),
            ));
        }

        if !update_project_task_has_meaningful_mutation(&params) {
            return Err(AppError::BadRequest(
                "update_project_task requires at least one meaningful change. Use sleep when nothing should change.".to_string(),
            ));
        }
        let task_key = params.task_key.clone();

        let board_item = self
            .update_project_task_board_item(
                task_key.clone(),
                params.status.clone(),
                update_project_task_resolved_priority(&params),
                update_project_task_metadata(),
                params
                    .schedule
                    .as_ref()
                    .map(normalize_schedule_params)
                    .as_ref()
                    .map(validate_schedule_params)
                    .transpose()?,
            )
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "update_project_task",
            "task_key": task_key,
            "updated": true,
            "board_item_id": board_item.id.to_string(),
        }))
    }

    pub(crate) async fn handle_assign_project_task(
        &mut self,
        params: AssignProjectTaskParams,
    ) -> Result<Value, AppError> {
        if !self.effective_is_coordinator_thread() {
            return Err(AppError::BadRequest(
                "assign_project_task is available only to the coordinator thread".to_string(),
            ));
        }

        if params.assignments.is_empty() {
            return Err(AppError::BadRequest(
                "assign_project_task requires at least one assignment. Use sleep when no routing change is needed.".to_string(),
            ));
        }

        let board_id = self.ensure_project_task_board_id().await?;
        let board_item =
            queries::GetProjectTaskBoardItemByTaskKeyQuery::new(board_id, params.task_key.clone())
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?
                .ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "Project task '{}' was not found in the current board",
                        params.task_key
                    ))
                })?;

        let changed = self
            .ensure_project_task_board_assignments(&board_item, Some(params.assignments))
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": "assign_project_task",
            "task_key": board_item.task_key,
            "updated": changed,
            "board_item_id": board_item.id.to_string(),
        }))
    }
}
