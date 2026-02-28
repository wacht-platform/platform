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

#[derive(Debug, Deserialize)]
struct NotifyParentParams {
    message: String,
    #[serde(default)]
    trigger_execution: bool,
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

    pub(super) async fn execute_notify_parent_tool(
        &self,
        tool: &AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: NotifyParentParams = parse_params(execution_params, "notify_parent")?;

        let context = self.ctx.get_context().await?;
        let parent_context_id = context.parent_context_id.ok_or_else(|| {
            AppError::BadRequest(
                "notify_parent is only available in child contexts (spawned by a parent agent)"
                    .to_string(),
            )
        })?;

        let conversation_id = self
            .app_state()
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate ID: {}", e)))?
            as i64;

        let relayed_message = format!(
            "[Message from child context #{}] {}",
            self.context_id(),
            params.message
        );

        let content = models::ConversationContent::UserMessage {
            message: relayed_message,
            sender_name: Some(format!("Child agent (context #{})", self.context_id())),
            files: None,
        };

        commands::CreateConversationCommand::new(
            conversation_id,
            parent_context_id,
            content,
            models::ConversationMessageType::UserMessage,
        )
        .execute(self.app_state())
        .await?;

        if params.trigger_execution {
            commands::PublishAgentExecutionCommand::new_message(
                self.agent().deployment_id,
                parent_context_id,
                None,
                Some(self.agent().name.clone()),
                conversation_id,
            )
            .execute(self.app_state())
            .await?;
        }

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "parent_context_id": parent_context_id,
            "message_delivered": true,
            "execution_triggered": params.trigger_execution
        }))
    }

    pub(super) async fn execute_get_child_messages_tool(
        &self,
        tool: &AiTool,
        _execution_params: &Value,
    ) -> Result<Value, AppError> {
        use queries::Query;

        let all_conversations = queries::GetLLMConversationHistoryQuery::new(self.context_id())
            .execute(self.app_state())
            .await
            .unwrap_or_default();

        let messages: Vec<Value> = all_conversations
            .iter()
            .filter(|conv| {
                conv.message_type == models::ConversationMessageType::UserMessage
                    && matches!(&conv.content, models::ConversationContent::UserMessage { sender_name: Some(name), .. } if name.starts_with("Child agent"))
            })
            .map(|conv| {
                let (message, sender) = match &conv.content {
                    models::ConversationContent::UserMessage { message, sender_name, .. } => {
                        (message.clone(), sender_name.clone().unwrap_or_default())
                    }
                    _ => (String::new(), String::new()),
                };
                serde_json::json!({
                    "sender": sender,
                    "message": message,
                    "received_at": conv.created_at.to_rfc3339(),
                })
            })
            .collect();

        let count = messages.len();
        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "messages": messages,
            "count": count
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
            InternalToolType::NotifyParent => {
                self.execute_notify_parent_tool(tool, execution_params)
                    .await
            }
            InternalToolType::GetChildMessages => {
                self.execute_get_child_messages_tool(tool, execution_params)
                    .await
            }
            _ => Err(AppError::Internal(
                "Unsupported swarm tool type".to_string(),
            )),
        }
    }
}
