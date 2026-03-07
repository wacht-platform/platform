use super::{parse_params, ToolExecutor};
use common::error::AppError;
use futures::TryStreamExt;
use models::{AiTool, InternalToolType};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::{sleep, Duration};

const SWARM_MAILBOX_BUCKET: &str = "agent_swarm_mailbox";
const SWARM_MAILBOX_TTL_SECS: u64 = 3600;
const EXECUTION_KV_BUCKET: &str = "agent_execution_kv";

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
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct SwarmMailboxMessage {
    #[serde(with = "models::utils::serde::i64_as_string")]
    message_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    parent_context_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    child_context_id: i64,
    sender: String,
    message: String,
    created_at: String,
}

fn mailbox_key(parent_context_id: i64, child_context_id: i64, message_id: i64) -> String {
    format!("p:{parent_context_id}:c:{child_context_id}:m:{message_id}")
}

fn mailbox_prefix(parent_context_id: i64) -> String {
    format!("p:{parent_context_id}:")
}

impl ToolExecutor {
    async fn get_or_create_swarm_mailbox(
        &self,
    ) -> Result<async_nats::jetstream::kv::Store, AppError> {
        match self
            .app_state()
            .nats_jetstream
            .get_key_value(SWARM_MAILBOX_BUCKET)
            .await
        {
            Ok(store) => Ok(store),
            Err(_) => {
                let created = self
                    .app_state()
                    .nats_jetstream
                    .create_key_value(async_nats::jetstream::kv::Config {
                        bucket: SWARM_MAILBOX_BUCKET.to_string(),
                        max_age: std::time::Duration::from_secs(SWARM_MAILBOX_TTL_SECS),
                        history: 1,
                        ..Default::default()
                    })
                    .await;
                match created {
                    Ok(store) => Ok(store),
                    Err(create_error) => self
                        .app_state()
                        .nats_jetstream
                        .get_key_value(SWARM_MAILBOX_BUCKET)
                        .await
                        .map_err(|get_error| {
                            AppError::Internal(format!(
                                "Failed to initialize swarm mailbox KV bucket: create error={}, get error={}",
                                create_error, get_error
                            ))
                        }),
                }
            }
        }
    }

    async fn current_execution_cursor(&self) -> Result<Option<i64>, AppError> {
        let kv = match self
            .app_state()
            .nats_jetstream
            .get_key_value(EXECUTION_KV_BUCKET)
            .await
        {
            Ok(store) => store,
            Err(_) => return Ok(None),
        };

        let Some(entry) = kv
            .entry(self.context_id().to_string())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read execution cursor: {}", e)))?
        else {
            return Ok(None);
        };

        let raw = String::from_utf8(entry.value.to_vec())
            .map_err(|e| AppError::Internal(format!("Invalid execution cursor encoding: {}", e)))?;
        if raw.starts_with("cancel:") {
            return Ok(None);
        }
        Ok(raw.parse::<i64>().ok())
    }

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
            status_update_id: Some(self.app_state().sf.next_id()? as i64),
            context_id: self.context_id(),
            deployment_id: self.agent().deployment_id,
            status_update: params.status,
            metadata: params.metadata,
        }
        .execute_with_db(self.app_state().db_router.writer())
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
        .execute_with_db(self.app_state().db_router.writer())
        .await?;

        let mailbox_message = SwarmMailboxMessage {
            message_id: conversation_id,
            parent_context_id,
            child_context_id: self.context_id(),
            sender: format!("Child agent (context #{})", self.context_id()),
            message: params.message,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let mailbox_key = mailbox_key(parent_context_id, self.context_id(), conversation_id);
        let mailbox_store = self.get_or_create_swarm_mailbox().await?;
        let payload = serde_json::to_vec(&mailbox_message).map_err(|e| {
            AppError::Internal(format!("Failed to serialize mailbox message: {}", e))
        })?;
        mailbox_store
            .put(mailbox_key, payload.into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write mailbox message: {}", e)))?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "parent_context_id": parent_context_id,
            "message_delivered": true
        }))
    }

    pub(super) async fn execute_get_child_messages_tool(
        &self,
        tool: &AiTool,
        _execution_params: &Value,
    ) -> Result<Value, AppError> {
        let mailbox_store = self.get_or_create_swarm_mailbox().await?;
        let prefix = mailbox_prefix(self.context_id());
        let execution_cursor = self.current_execution_cursor().await?;

        let keys_stream = mailbox_store
            .keys()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to list mailbox keys: {}", e)))?;
        let keys: Vec<String> = keys_stream
            .try_collect()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to collect mailbox keys: {}", e)))?;

        let mut messages_with_id: Vec<(i64, Value)> = Vec::new();
        for key in keys {
            if !key.starts_with(&prefix) {
                continue;
            }
            let message_id = key
                .split(":m:")
                .nth(1)
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or_default();
            if let Some(cursor) = execution_cursor {
                if message_id <= cursor {
                    continue;
                }
            }

            let entry = mailbox_store
                .entry(key)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to read mailbox entry: {}", e)))?;
            let Some(entry) = entry else {
                continue;
            };
            let payload =
                serde_json::from_slice::<SwarmMailboxMessage>(&entry.value).map_err(|e| {
                    AppError::Internal(format!("Failed to parse mailbox message payload: {}", e))
                })?;
            messages_with_id.push((
                payload.message_id,
                serde_json::json!({
                    "sender": payload.sender,
                    "message": payload.message,
                    "received_at": payload.created_at,
                    "child_context_id": payload.child_context_id.to_string(),
                }),
            ));
        }

        messages_with_id.sort_by_key(|(message_id, _)| *message_id);
        let messages: Vec<Value> = messages_with_id.into_iter().map(|(_, v)| v).collect();

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
