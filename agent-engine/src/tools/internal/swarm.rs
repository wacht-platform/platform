use super::{parse_params, ToolExecutor};
use commands::Command;
use common::error::AppError;
use models::{AiTool, InternalToolType};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::{sleep, Duration};

#[derive(Debug, Deserialize)]
struct UpdateStatusParams {
    status: String,
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SleepParams {
    duration_ms: u64,
    #[serde(default)]
    reason: Option<String>,
}

impl ToolExecutor {
    pub(super) async fn execute_sleep_tool(
        &self,
        tool: &AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: SleepParams = parse_params(execution_params, "sleep")?;
        let bounded_ms = params.duration_ms.min(10_000);
        sleep(Duration::from_millis(bounded_ms)).await;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "slept_ms": bounded_ms,
            "requested_ms": params.duration_ms,
            "reason": params.reason
        }))
    }

    pub(super) async fn execute_update_status_tool(
        &self,
        tool: &AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: UpdateStatusParams = parse_params(execution_params, "update_status")?;

        commands::PostStatusUpdateCommand {
            context_id: self.context_id(),
            deployment_id: self.agent().deployment_id,
            status_update: params.status,
            metadata: params.metadata,
        }
        .execute(self.app_state())
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "message": "Status update posted successfully"
        }))
    }

    pub(super) async fn execute_swarm_tool(
        &self,
        tool: &AiTool,
        tool_type: InternalToolType,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        match tool_type {
            InternalToolType::GetChildStatus => {
                let request = parse_params::<crate::swarm::GetChildStatusRequest>(
                    execution_params,
                    "get_child_status",
                )?;

                crate::swarm::get_child_status(
                    self.app_state(),
                    self.agent().deployment_id,
                    self.context_id(),
                    &tool.name,
                    request,
                )
                .await
            }
            InternalToolType::SpawnContext => {
                let _request =
                    parse_params::<serde_json::Value>(execution_params, "spawn_context")?;
                Err(AppError::BadRequest(
                    "spawn_context is no longer supported. Use spawn_context_execution with `target_context_id` and `instructions`."
                        .to_string(),
                ))
            }
            InternalToolType::SpawnControl => {
                let request = parse_params::<crate::swarm::SpawnControlRequest>(
                    execution_params,
                    "spawn_control",
                )?;

                crate::swarm::spawn_control(
                    self.app_state(),
                    self.agent().deployment_id,
                    self.context_id(),
                    &tool.name,
                    request,
                )
                .await
            }
            InternalToolType::GetCompletionSummary => {
                let request = parse_params::<crate::swarm::GetCompletionSummaryRequest>(
                    execution_params,
                    "get_completion_summary",
                )?;

                crate::swarm::completion_summary(
                    self.app_state(),
                    self.agent().deployment_id,
                    self.context_id(),
                    &tool.name,
                    request,
                )
                .await
            }
            _ => Err(AppError::Internal(
                "Unsupported swarm tool type".to_string(),
            )),
        }
    }
}
