use crate::agentic::AgentExecutor;
use async_nats::jetstream;
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::AiAgent;
use shared::state::AppState;

pub struct AgentHandler {
    pub app_state: AppState,
}

#[derive(Debug, Clone)]
pub struct ExecutionRequest {
    pub agent: AiAgent,
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
            &self.app_state,
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
        let mut watch = match kv.watch(context_key.clone()).await {
            Err(err) => return Err(AppError::Internal(err.to_string())),
            Ok(watch) => watch,
        };

        let (sender, mut receiver) = tokio::sync::mpsc::channel::<StreamEvent>(10);
        let jetstream = self.app_state.nats_jetstream.clone();

        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                match message {
                    StreamEvent::Token(token) => {
                        let v = jetstream
                            .publish(
                                format!("agent_execution_stream.msg:{}", execution_id),
                                token.into(),
                            )
                            .await;
                    }
                    StreamEvent::Message(agent_execution_context_message) => todo!(),
                }
            }
        });

        tokio::join!(
            agent_executor.execute_with_streaming(&request.user_message, sender),
            kv.put(context_key.clone(), execution_id.to_string().into())
        );

        let _ = kv.delete(context_key).await;

        Ok(())
    }
}
