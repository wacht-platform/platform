use super::ToolExecutor;
use crate::filesystem::AgentFilesystem;

use common::error::AppError;
use models::{AiTool, UseExternalServiceToolConfiguration, UseExternalServiceToolType};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Copy)]
enum WhatsAppAction {
    SendMessage,
    GetMessage,
    MarkRead,
}

#[derive(Debug, Deserialize)]
struct WhatsAppSendMessageParams {
    to: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct WhatsAppMessageIdParams {
    message_id: String,
}

fn parse_external_params<T: DeserializeOwned>(
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

impl ToolExecutor {
    pub(super) async fn execute_external_service_tool(
        &self,
        _tool: &AiTool,
        config: &UseExternalServiceToolConfiguration,
        execution_params: &Value,
        _context_title: &str,
        _filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        match config.service_type {
            UseExternalServiceToolType::WhatsAppSendMessage => {
                self.execute_whatsapp_command(WhatsAppAction::SendMessage, execution_params)
                    .await
            }
            UseExternalServiceToolType::WhatsAppGetMessage => {
                self.execute_whatsapp_command(WhatsAppAction::GetMessage, execution_params)
                    .await
            }
            UseExternalServiceToolType::WhatsAppMarkRead => {
                self.execute_whatsapp_command(WhatsAppAction::MarkRead, execution_params)
                    .await
            }
            UseExternalServiceToolType::TeamsListUsers
            | UseExternalServiceToolType::TeamsSearchUsers
            | UseExternalServiceToolType::TeamsSendContextMessage
            | UseExternalServiceToolType::TeamsListMessages
            | UseExternalServiceToolType::TeamsGetMeetingRecording
            | UseExternalServiceToolType::TeamsTranscribeMeeting
            | UseExternalServiceToolType::TeamsSaveAttachment
            | UseExternalServiceToolType::TeamsListContexts => Err(AppError::BadRequest(
                "Teams tools are removed from the current runtime".to_string(),
            )),
            UseExternalServiceToolType::ClickUpCreateTask
            | UseExternalServiceToolType::ClickUpCreateList
            | UseExternalServiceToolType::ClickUpUpdateTask
            | UseExternalServiceToolType::ClickUpAddComment
            | UseExternalServiceToolType::ClickUpGetTask
            | UseExternalServiceToolType::ClickUpGetSpaceLists
            | UseExternalServiceToolType::ClickUpGetSpaces
            | UseExternalServiceToolType::ClickUpGetTeams
            | UseExternalServiceToolType::ClickUpGetCurrentUser
            | UseExternalServiceToolType::ClickUpGetTasks
            | UseExternalServiceToolType::ClickUpSearchTasks
            | UseExternalServiceToolType::ClickUpTaskAddAttachment => Err(AppError::BadRequest(
                "ClickUp tools are removed from the current runtime".to_string(),
            )),
            UseExternalServiceToolType::McpCallTool => Err(AppError::BadRequest(
                "MCP tools are removed from the current runtime".to_string(),
            )),
        }
    }

    async fn execute_whatsapp_command(
        &self,
        action: WhatsAppAction,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        match action {
            WhatsAppAction::SendMessage => {
                let params: WhatsAppSendMessageParams =
                    parse_external_params(execution_params, "whatsapp_send_message")?;
                let _ = params.message;

                let message_id = self.ctx.app_state.sf.next_id().unwrap_or(0);
                Ok(serde_json::json!({
                    "success": true,
                    "message": "WhatsApp message sent (placeholder)",
                    "to": params.to,
                    "message_id": format!("wa_{}", message_id),
                }))
            }
            WhatsAppAction::GetMessage => {
                let params: WhatsAppMessageIdParams =
                    parse_external_params(execution_params, "whatsapp_get_message")?;

                Ok(serde_json::json!({
                    "message_id": params.message_id,
                    "from": "1234567890",
                    "to": "0987654321",
                    "text": "Sample message",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                }))
            }
            WhatsAppAction::MarkRead => {
                let params: WhatsAppMessageIdParams =
                    parse_external_params(execution_params, "whatsapp_mark_read")?;

                Ok(serde_json::json!({
                    "success": true,
                    "message_id": params.message_id,
                    "status": "read",
                }))
            }
        }
    }
}
