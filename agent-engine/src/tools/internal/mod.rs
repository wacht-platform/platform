mod filesystem;
mod memory;
mod swarm;

use super::ToolExecutor;
use common::error::AppError;
use models::{AiTool, InternalToolConfiguration, InternalToolType};
use serde::de::DeserializeOwned;
use serde_json::Value;

impl ToolExecutor {
    pub(super) async fn execute_internal_tool(
        &self,
        tool: &AiTool,
        config: &InternalToolConfiguration,
        execution_params: &Value,
        filesystem: &crate::filesystem::AgentFilesystem,
        shell: &crate::filesystem::shell::ShellExecutor,
    ) -> Result<Value, AppError> {
        tracing::info!(
            tool_name = %tool.name,
            params = %execution_params,
            "Executing internal tool"
        );

        match config.tool_type {
            InternalToolType::ReadFile
            | InternalToolType::ReadImage
            | InternalToolType::WriteFile
            | InternalToolType::ListDirectory
            | InternalToolType::SearchFiles
            | InternalToolType::ExecuteCommand => {
                self.execute_filesystem_tool(
                    tool,
                    config.tool_type.clone(),
                    execution_params,
                    filesystem,
                    shell,
                )
                .await
            }
            InternalToolType::SaveMemory => {
                self.execute_save_memory_tool(tool, execution_params).await
            }
            InternalToolType::Sleep => self.execute_sleep_tool(tool, execution_params).await,
            InternalToolType::UpdateStatus => {
                self.execute_update_status_tool(tool, execution_params)
                    .await
            }
            InternalToolType::GetChildStatus
            | InternalToolType::SpawnContext
            | InternalToolType::SpawnControl
            | InternalToolType::GetCompletionSummary
            | InternalToolType::SwitchExecutionMode
            | InternalToolType::UpdateTaskBoard
            | InternalToolType::ExitSupervisorMode
            | InternalToolType::NotifyParent
            | InternalToolType::GetChildMessages => {
                self.execute_swarm_tool(tool, config.tool_type.clone(), execution_params)
                    .await
            }
        }
    }
}

pub(super) fn parse_params<T: DeserializeOwned>(
    execution_params: &Value,
    tool_name: &str,
) -> Result<T, AppError> {
    let normalized = if execution_params.is_null() {
        serde_json::json!({})
    } else {
        execution_params.clone()
    };

    serde_json::from_value::<T>(normalized)
        .map_err(|e| AppError::BadRequest(format!("Invalid {tool_name} params: {e}")))
}
