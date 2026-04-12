use super::ToolExecutor;
use crate::filesystem::AgentFilesystem;
use crate::runtime::task_workspace::compute_task_journal_hash;
use common::error::AppError;
use dto::json::agent_executor::{
    TaskGraphAddDependencyParams, TaskGraphAddNodeParams, TaskGraphCompleteNodeParams,
    TaskGraphFailNodeParams, TaskGraphMarkCompletedParams, TaskGraphMarkFailedParams,
    TaskGraphNodeTargetParams,
};
use queries::ListAssignmentsForThreadQuery;
use serde_json::Value;

async fn validate_handoff_path(
    filesystem: &AgentFilesystem,
    handoff_path: &str,
) -> Result<(), AppError> {
    if !handoff_path.starts_with("/task/handoffs/") {
        return Err(AppError::BadRequest(
            "handoff_path must be inside /task/handoffs/".to_string(),
        ));
    }
    let handoff_full_path = filesystem.resolve_path_public(handoff_path)?;
    if tokio::fs::metadata(&handoff_full_path).await.is_err() {
        return Err(AppError::BadRequest(format!(
            "Task graph handoff file does not exist: {}",
            handoff_path
        )));
    }
    Ok(())
}

async fn validate_task_journal_guard(
    executor: &ToolExecutor,
    filesystem: &AgentFilesystem,
) -> Result<(), AppError> {
    let thread = executor.ctx.get_thread().await?;
    let start_hash = thread
        .execution_state
        .as_ref()
        .and_then(|state| state.task_journal_start_hash.clone())
        .ok_or_else(|| {
            AppError::Internal("Task journal start hash missing for task run".to_string())
        })?;
    let current_hash = compute_task_journal_hash(filesystem).await?;

    if current_hash == start_hash {
        return Err(AppError::BadRequest(
            "Update /task/JOURNAL.md with write_file or edit_file before completing this task stage.".to_string(),
        ));
    }

    Ok(())
}

impl ToolExecutor {
    async fn current_assignment_board_item_id(&self) -> Result<Option<i64>, AppError> {
        let assignments = ListAssignmentsForThreadQuery::new(self.thread_id())
            .execute_with_db(self.app_state().db_router.writer())
            .await?;

        let status_rank = |status: &str| match status {
            models::project_task_board::assignment_status::IN_PROGRESS => 10,
            models::project_task_board::assignment_status::CLAIMED => 20,
            models::project_task_board::assignment_status::AVAILABLE => 30,
            models::project_task_board::assignment_status::PENDING => 40,
            models::project_task_board::assignment_status::BLOCKED => 50,
            _ => 60,
        };

        Ok(assignments
            .into_iter()
            .filter(|assignment| status_rank(&assignment.status) < 60)
            .min_by_key(|assignment| {
                (
                    status_rank(&assignment.status),
                    assignment.assignment_order,
                    assignment.created_at,
                )
            })
            .map(|assignment| assignment.board_item_id))
    }

    async fn ensure_task_graph(&self) -> Result<models::ThreadTaskGraph, AppError> {
        let mut command = commands::EnsureThreadTaskGraphCommand::new(
            self.app_state().sf.next_id()? as i64,
            self.agent().deployment_id,
            self.thread_id(),
        );

        if let Some(board_item_id) = self.current_assignment_board_item_id().await? {
            command = command.with_board_item_id(board_item_id);
        }

        command
            .execute_with_db(self.app_state().db_router.writer())
            .await
    }

    pub(super) async fn execute_task_graph_add_node(
        &self,
        tool: &models::AiTool,
        params: TaskGraphAddNodeParams,
    ) -> Result<Value, AppError> {
        let graph = self.ensure_task_graph().await?;

        let node = commands::CreateThreadTaskNodeCommand {
            id: self.app_state().sf.next_id()? as i64,
            graph_id: graph.id,
            board_item_id: graph.board_item_id,
            title: params.title,
            description: params.description,
            max_retries: params.max_retries.unwrap_or(2),
            input: params.input,
        }
        .execute_with_db(self.app_state().db_router.writer())
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "created_graph_id": graph.id.to_string(),
            "created_node_id": node.id.to_string(),
            "graph_id": graph.id.to_string(),
            "node": node
        }))
    }

    pub(super) async fn execute_task_graph_add_dependency(
        &self,
        tool: &models::AiTool,
        params: TaskGraphAddDependencyParams,
    ) -> Result<Value, AppError> {
        let graph = self.ensure_task_graph().await?;
        let from_node_id = params
            .from_node_id
            .ok_or_else(|| {
                AppError::BadRequest(
                    "task_graph_add_dependency requires resolved `from_node_id`".to_string(),
                )
            })?
            .into_inner();
        let to_node_id = params
            .to_node_id
            .ok_or_else(|| {
                AppError::BadRequest(
                    "task_graph_add_dependency requires resolved `to_node_id`".to_string(),
                )
            })?
            .into_inner();

        commands::AddThreadTaskDependencyCommand {
            graph_id: graph.id,
            from_node_id,
            to_node_id,
        }
        .execute_with_db(self.app_state().db_router.writer())
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "graph_id": graph.id.to_string(),
            "from_node_id": from_node_id.to_string(),
            "to_node_id": to_node_id.to_string()
        }))
    }

    pub(super) async fn execute_task_graph_mark_in_progress(
        &self,
        tool: &models::AiTool,
        params: TaskGraphNodeTargetParams,
    ) -> Result<Value, AppError> {
        let graph = self.ensure_task_graph().await?;
        let node_id = params
            .node_id
            .ok_or_else(|| {
                AppError::BadRequest(
                    "task_graph_mark_in_progress requires resolved `node_id`".to_string(),
                )
            })?
            .into_inner();

        let node = commands::MarkThreadTaskNodeInProgressCommand {
            graph_id: graph.id,
            node_id,
        }
        .execute_with_db(self.app_state().db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("Task node not found".to_string()))?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "graph_id": graph.id.to_string(),
            "node": node
        }))
    }

    pub(super) async fn execute_task_graph_complete_node(
        &self,
        tool: &models::AiTool,
        params: TaskGraphCompleteNodeParams,
    ) -> Result<Value, AppError> {
        let graph = self.ensure_task_graph().await?;
        let node_id = params
            .target
            .node_id
            .ok_or_else(|| {
                AppError::BadRequest(
                    "task_graph_complete_node requires resolved `node_id`".to_string(),
                )
            })?
            .into_inner();

        let node = commands::CompleteThreadTaskNodeCommand {
            graph_id: graph.id,
            node_id,
            output: params.output,
        }
        .execute_with_db(self.app_state().db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("Task node not found".to_string()))?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "graph_id": graph.id.to_string(),
            "node": node
        }))
    }

    pub(super) async fn execute_task_graph_fail_node(
        &self,
        tool: &models::AiTool,
        params: TaskGraphFailNodeParams,
    ) -> Result<Value, AppError> {
        let graph = self.ensure_task_graph().await?;
        let node_id = params
            .target
            .node_id
            .ok_or_else(|| {
                AppError::BadRequest("task_graph_fail_node requires resolved `node_id`".to_string())
            })?
            .into_inner();

        let node = commands::FailThreadTaskNodeCommand {
            graph_id: graph.id,
            node_id,
            error: params.error,
        }
        .execute_with_db(self.app_state().db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("Task node not found".to_string()))?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "graph_id": graph.id.to_string(),
            "node": node
        }))
    }

    pub(super) async fn execute_task_graph_mark_completed(
        &self,
        tool: &models::AiTool,
        params: TaskGraphMarkCompletedParams,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        validate_handoff_path(filesystem, &params.handoff_path).await?;
        validate_task_journal_guard(self, filesystem).await?;

        let graph = self.ensure_task_graph().await?;
        let updated = commands::MarkThreadTaskGraphCompletedCommand { graph_id: graph.id }
            .execute_with_db(self.app_state().db_router.writer())
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "graph": updated,
            "handoff_path": params.handoff_path
        }))
    }

    pub(super) async fn execute_task_graph_mark_failed(
        &self,
        tool: &models::AiTool,
        params: TaskGraphMarkFailedParams,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        validate_handoff_path(filesystem, &params.handoff_path).await?;
        validate_task_journal_guard(self, filesystem).await?;

        let graph = self.ensure_task_graph().await?;
        let updated = commands::MarkThreadTaskGraphFailedCommand { graph_id: graph.id }
            .execute_with_db(self.app_state().db_router.writer())
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "graph": updated,
            "handoff_path": params.handoff_path
        }))
    }
}
