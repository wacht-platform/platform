use commands::{Command, TriggerWebhookEventCommand};
use common::state::AppState;
use serde::{Deserialize, Serialize};
use tracing::error;

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentStreamLogTask {
    pub context_id: i64,
    pub deployment_id: i64,
    pub message_type: String,
    pub payload: serde_json::Value,
}

pub async fn log_agent_stream_message(
    app_state: &AppState,
    task: AgentStreamLogTask,
) -> Result<String, anyhow::Error> {
    let webhook_event = match task.message_type.as_str() {
        "conversation_message" => "execution_context.message",
        "platform_event" => "execution_context.platform_event",
        "platform_function" => "execution_context.platform_function",
        "user_input_request" => "execution_context.user_input_request",
        _ => "execution_context.message",
    };
    
    let webhook_payload = serde_json::json!({
        "context_id": task.context_id,
        "deployment_id": task.deployment_id,
        "message_type": task.message_type,
        "data": task.payload,
        "timestamp": chrono::Utc::now(),
    });
    
    let trigger_command = TriggerWebhookEventCommand::new(
        task.deployment_id,
        task.deployment_id.to_string(),
        webhook_event.to_string(),
        webhook_payload,
    );
    
    if let Err(e) = trigger_command.execute(app_state).await {
        error!(
            "Failed to trigger webhook for event {}: {}",
            webhook_event, e
        );
    }
    
    Ok(format!(
        "Processed {} message for context {}",
        task.message_type, task.context_id
    ))
}