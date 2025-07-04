use crate::agentic::AgentExecutor;
use async_nats::{HeaderMap, jetstream};
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
        let mut agent_executor = AgentExecutor::new(
            request.agent,
            request.deployment_id,
            request.context_id,
            self.app_state.clone(),
        )
        .await?;
        let execution_id = self.app_state.sf.next_id()? as i64;
        let context_key = format!("{}", request.context_id);

        let kv = match self
            .app_state
            .nats_jetstream
            .create_key_value(jetstream::kv::Config {
                bucket: "agent_execution_kv".to_string(),
                ..Default::default()
            })
            .await
        {
            Err(err) => return Err(AppError::Internal(err.to_string())),
            Ok(kv) => kv,
        };
        let watch = match kv.watch(context_key.clone()).await {
            Err(err) => return Err(AppError::Internal(err.to_string())),
            Ok(watch) => watch,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::channel::<StreamEvent>(10);
        let jetstream = self.app_state.nats_jetstream.clone();

        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    StreamEvent::Token(token, message_id) => {
                        let mut headers = HeaderMap::new();
                        headers.append("message_id", message_id);
                        let _ = jetstream
                            .publish_with_headers(
                                format!("agent_execution_stream.msg:{}", execution_id),
                                headers,
                                token.into(),
                            )
                            .await;
                    }
                    StreamEvent::Message(_) => todo!(),
                }
            }
        });

        match kv
            .put(context_key.clone(), execution_id.to_string().into())
            .await
        {
            Ok(_) => {
                let _ = agent_executor
                    .execute_with_streaming(&request.user_message, sender)
                    .await;
            }
            Err(_) => (),
        };

        let _ = kv.delete(context_key).await;

        Ok(())
    }
}
