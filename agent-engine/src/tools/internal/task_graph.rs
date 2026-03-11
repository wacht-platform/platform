use super::{parse_params, ToolExecutor};
use crate::filesystem::AgentFilesystem;
use common::error::AppError;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct TaskGraphAddNodeParams {
    title: String,
    description: Option<String>,
    max_retries: Option<i32>,
    input: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct TaskGraphAddDependencyParams {
    from_node_id: Value,
    to_node_id: Value,
}

#[derive(Debug, Deserialize)]
struct TaskGraphCompleteNodeParams {
    node_id: Value,
    output: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct TaskGraphFailNodeParams {
    node_id: Value,
    error: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct TaskGraphMarkCompletedParams {
    handoff_path: String,
}

fn parse_i64_field(value: &Value, field_name: &str) -> Result<i64, AppError> {
    if let Some(s) = value.as_str() {
        return s
            .parse::<i64>()
            .map_err(|_| AppError::BadRequest(format!("Invalid {} '{}'", field_name, s)));
    }
    if let Some(n) = value.as_i64() {
        return Ok(n);
    }
    Err(AppError::BadRequest(format!(
        "{} must be string or integer",
        field_name
    )))
}

impl ToolExecutor {
    async fn ensure_task_graph(&self) -> Result<models::ExecutionTaskGraph, AppError> {
        commands::EnsureExecutionTaskGraphCommand::new(
            self.app_state().sf.next_id()? as i64,
            self.agent().deployment_id,
            self.context_id(),
        )
        .execute_with_db(self.app_state().db_router.writer())
        .await
    }

    pub(super) async fn execute_task_graph_add_node_tool(
        &self,
        tool: &models::AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: TaskGraphAddNodeParams = parse_params(execution_params, "task_graph_add_node")?;
        let graph = self.ensure_task_graph().await?;

        let node = commands::CreateExecutionTaskNodeCommand {
            id: self.app_state().sf.next_id()? as i64,
            graph_id: graph.id,
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
            "graph_id": graph.id.to_string(),
            "node": node
        }))
    }

    pub(super) async fn execute_task_graph_add_dependency_tool(
        &self,
        tool: &models::AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: TaskGraphAddDependencyParams =
            parse_params(execution_params, "task_graph_add_dependency")?;
        let graph = self.ensure_task_graph().await?;
        let from_node_id = parse_i64_field(&params.from_node_id, "from_node_id")?;
        let to_node_id = parse_i64_field(&params.to_node_id, "to_node_id")?;

        commands::AddExecutionTaskDependencyCommand {
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

    pub(super) async fn execute_task_graph_mark_in_progress_tool(
        &self,
        tool: &models::AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        #[derive(Debug, Deserialize)]
        struct TaskGraphMarkInProgressParams {
            node_id: Value,
        }

        let params: TaskGraphMarkInProgressParams =
            parse_params(execution_params, "task_graph_mark_in_progress")?;
        let graph = self.ensure_task_graph().await?;
        let node_id = parse_i64_field(&params.node_id, "node_id")?;

        let node = commands::MarkExecutionTaskNodeInProgressCommand {
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

    pub(super) async fn execute_task_graph_complete_node_tool(
        &self,
        tool: &models::AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: TaskGraphCompleteNodeParams =
            parse_params(execution_params, "task_graph_complete_node")?;
        let graph = self.ensure_task_graph().await?;
        let node_id = parse_i64_field(&params.node_id, "node_id")?;

        let node = commands::CompleteExecutionTaskNodeCommand {
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

    pub(super) async fn execute_task_graph_fail_node_tool(
        &self,
        tool: &models::AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: TaskGraphFailNodeParams =
            parse_params(execution_params, "task_graph_fail_node")?;
        let graph = self.ensure_task_graph().await?;
        let node_id = parse_i64_field(&params.node_id, "node_id")?;

        let node = commands::FailExecutionTaskNodeCommand {
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

    pub(super) async fn execute_task_graph_mark_completed_tool(
        &self,
        tool: &models::AiTool,
        execution_params: &Value,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        let params: TaskGraphMarkCompletedParams =
            parse_params(execution_params, "task_graph_mark_completed")?;
        if !params.handoff_path.starts_with("/workspace/") {
            return Err(AppError::BadRequest(
                "handoff_path must be inside /workspace/".to_string(),
            ));
        }
        let handoff_full_path = filesystem.resolve_path_public(&params.handoff_path)?;
        if tokio::fs::metadata(&handoff_full_path).await.is_err() {
            return Err(AppError::BadRequest(format!(
                "Cannot mark task graph completed because handoff file does not exist: {}",
                params.handoff_path
            )));
        }

        let graph = self.ensure_task_graph().await?;
        let updated = commands::MarkExecutionTaskGraphCompletedCommand { graph_id: graph.id }
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
