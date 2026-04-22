use super::ToolExecutor;
use common::error::AppError;
use dto::json::agent_executor::{
    TaskGraphAddDependencyParams, TaskGraphAddNodeParams, TaskGraphCompleteNodeParams,
    TaskGraphFailNodeParams, TaskGraphNodeTargetParams, TaskGraphResetParams,
};
use serde_json::Value;

impl ToolExecutor {
    async fn ensure_task_graph(&self) -> Result<models::ThreadTaskGraph, AppError> {
        let mut command = commands::EnsureThreadTaskGraphCommand::new(
            self.app_state().sf.next_id()? as i64,
            self.agent().deployment_id,
            self.thread_id(),
        );

        if let Some(board_item_id) = self.active_board_item_id() {
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
        let from_node_id = params.from_node_id.into_inner();
        let to_node_id = params.to_node_id.into_inner();

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
        let node_id = params.node_id.into_inner();

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
        let node_id = params.target.node_id.into_inner();

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
        let node_id = params.target.node_id.into_inner();

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

    pub(super) async fn execute_task_graph_reset(
        &self,
        tool: &models::AiTool,
        params: TaskGraphResetParams,
    ) -> Result<Value, AppError> {
        let graph = self.ensure_task_graph().await?;
        let updated = commands::CancelThreadTaskGraphCommand { graph_id: graph.id }
            .execute_with_db(self.app_state().db_router.writer())
            .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "graph": updated,
            "reason": params.reason,
            "note": "Previous graph cancelled. Call task_graph_add_node next to start a fresh plan.",
        }))
    }
}
