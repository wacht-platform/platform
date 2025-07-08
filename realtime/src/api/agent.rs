use crate::agentic::AgentExecutor;
use async_nats::HeaderMap;
use llm::chat::ChatRole;
use shared::commands::{Command, CreateConversationCommand};
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::AiAgentWithFeatures;
use shared::state::AppState;

pub struct AgentHandler {
    pub app_state: AppState,
}

#[derive(Clone)]
pub struct ExecutionRequest {
    pub agent: AiAgentWithFeatures,
    pub deployment_id: i64,
    pub user_message: String,
    pub context_id: i64,
}

impl AgentHandler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn execute_agent_streaming(&self, request: ExecutionRequest) -> Result<(), AppError> {
        let (sender, mut receiver) = tokio::sync::mpsc::channel::<StreamEvent>(10);

        let mut agent_executor = AgentExecutor::new(
            request.agent,
            request.deployment_id,
            request.context_id,
            self.app_state.clone(),
            sender.clone(),
        )
        .await?;
        let execution_id = self.app_state.sf.next_id()? as i64;
        let context_key = format!("{}", request.context_id);

        let kv = match self
            .app_state
            .nats_jetstream
            .get_key_value("agent_execution_kv".to_string())
            .await
        {
            Err(err) => return Err(AppError::Internal(err.to_string())),
            Ok(kv) => kv,
        };
        let _watch = match kv.watch(context_key.clone()).await {
            Err(err) => return Err(AppError::Internal(err.to_string())),
            Ok(watch) => watch,
        };

        let jetstream = self.app_state.nats_jetstream.clone();

        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    StreamEvent::Token(token) => {
                        let _ = jetstream
                            .publish(
                                format!("agent_execution_stream.msg:{}", execution_id),
                                token.into(),
                            )
                            .await;
                    }
                    StreamEvent::PlatformEvent(event_label, event_data) => {
                        let mut headers = HeaderMap::new();
                        headers.append("event_type", "platform_event");
                        headers.append("event_label", event_label);
                        let _ = jetstream
                            .publish_with_headers(
                                format!("agent_execution_stream.event:{}", execution_id),
                                headers,
                                serde_json::to_vec(&event_data).unwrap_or_default().into(),
                            )
                            .await;
                    }
                    StreamEvent::PlatformFunction(function_name, result) => {
                        let mut headers = HeaderMap::new();
                        headers.append("event_type", "platform_function");
                        headers.append("function_name", function_name);
                        let _ = jetstream
                            .publish_with_headers(
                                format!("agent_execution_stream.function:{}", execution_id),
                                headers,
                                serde_json::to_vec(&result).unwrap_or_default().into(),
                            )
                            .await;
                    }
                }
            }
        });

        match kv
            .put(context_key.clone(), execution_id.to_string().into())
            .await
        {
            Ok(_) => {
                agent_executor
                    .execute_with_streaming(&request.user_message)
                    .await
                    .unwrap();
            }
            Err(_) => (),
        };

        let message = agent_executor
            .conversations
            .iter()
            .rev()
            .position(|msg| msg.role == ChatRole::User)
            .map(|pos| {
                let start_idx = agent_executor.conversations.len() - pos - 1;
                agent_executor.conversations[start_idx..]
                    .iter()
                    .map(|v| v.content.clone())
                    .collect()
            })
            .unwrap_or_else(Vec::new)
            .join("\n");

        CreateConversationCommand::new(
            execution_id,
            request.context_id,
            message.clone(),
            "agent_response".to_string(),
        )
        .execute(&self.app_state)
        .await?;

        let _ = kv.delete(context_key).await;

        Ok(())
    }
}
