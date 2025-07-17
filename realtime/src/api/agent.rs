use crate::agentic::AgentExecutor;
use chrono::Utc;
use futures::StreamExt;
use shared::dto::json::StreamEvent;
use shared::error::AppError;
use shared::models::AiAgentWithFeatures;
use shared::state::AppState;
use tracing::{error, warn};

pub struct AgentHandler {
    app_state: AppState,
}

#[derive(Clone)]
pub struct ExecutionRequest {
    pub agent: AiAgentWithFeatures,
    pub user_message: String,
    pub context_id: i64,
}

impl AgentHandler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn execute_agent_streaming(&self, request: ExecutionRequest) -> Result<(), AppError> {
        let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(100);
        let execution_id = self.app_state.sf.next_id()? as i64;
        let context_key = request.context_id.to_string();

        let agent_executor = AgentExecutor::new(
            request.agent,
            request.context_id,
            self.app_state.clone(),
            sender,
        )
        .await?;

        let kv = self.get_key_value_store().await?;
        let watch = self.create_watcher(&kv, &context_key).await?;
        self.spawn_message_publisher(receiver, context_key.clone());
        
        // Yield to ensure the spawned task starts running
        tokio::task::yield_now().await;

        let execution_result = self
            .run_agent_execution(
                &kv,
                &context_key,
                execution_id,
                agent_executor,
                &request.user_message,
                watch,
            )
            .await;

        if let Err(e) = kv.delete(&context_key).await {
            warn!("Failed to delete execution key: {}", e);
        }

        execution_result
    }

    async fn get_key_value_store(&self) -> Result<async_nats::jetstream::kv::Store, AppError> {
        self.app_state
            .nats_jetstream
            .get_key_value("agent_execution_kv")
            .await
            .map_err(|err| AppError::Internal(format!("Failed to get key-value store: {}", err)))
    }

    async fn create_watcher(
        &self,
        kv: &async_nats::jetstream::kv::Store,
        context_key: &str,
    ) -> Result<async_nats::jetstream::kv::Watch, AppError> {
        kv.watch(context_key)
            .await
            .map_err(|err| AppError::Internal(format!("Failed to create watcher: {}", err)))
    }

    fn spawn_message_publisher(
        &self,
        mut receiver: tokio::sync::mpsc::Receiver<StreamEvent>,
        context_key: String,
    ) {
        let jetstream = self.app_state.nats_jetstream.clone();
        println!("Spawning message publisher task at: {}", Utc::now());

        tokio::spawn(async move {
            println!("Message publisher task started at: {}", Utc::now());
            let mut message_count = 0;
            while let Some(message) = receiver.recv().await {
                message_count += 1;
                let receive_time = Utc::now();
                println!("Message #{} received from channel at: {}", message_count, receive_time);
                
                // Check channel queue size
                println!("Channel queue size after receive: {}", receiver.len());
                
                if let Err(e) = publish_stream_event(&jetstream, &context_key, message).await {
                    error!("Failed to publish message: {}", e);
                } else {
                    println!(
                        "Total channel->NATS time: {}ms",
                        (Utc::now() - receive_time).num_milliseconds()
                    );
                }
            }
            println!("Channel receiver loop ended");
        });
    }

    async fn run_agent_execution(
        &self,
        kv: &async_nats::jetstream::kv::Store,
        context_key: &str,
        execution_id: i64,
        mut agent_executor: AgentExecutor,
        user_message: &str,
        mut watch: async_nats::jetstream::kv::Watch,
    ) -> Result<(), AppError> {
        kv.put(context_key, execution_id.to_string().into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to store execution ID: {}", e)))?;

        tokio::select! {
            result = agent_executor.execute_with_streaming(user_message) => {
                result
            }
            _ = watch_for_cancellation(&mut watch, execution_id) => {
                warn!("Execution cancelled for context {}", context_key);
                Ok(())
            }
        }
    }
}

async fn publish_stream_event(
    jetstream: &async_nats::jetstream::Context,
    context_key: &str,
    event: StreamEvent,
) -> Result<(), AppError> {
    let StreamEvent::ConversationMessage(conversation_content) = event else {
        return Ok(());
    };

    let subject = format!("agent_execution_stream.conversation:{}", context_key);
    let payload = serde_json::to_vec(&conversation_content)
        .map_err(|e| AppError::Internal(format!("Failed to serialize message: {}", e)))?;

    let start = Utc::now();
    println!("Publishing to NATS subject: {} at {}", subject, start);
    
    jetstream
        .publish(subject, payload.into())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {}", e)))?;

    println!(
        "NATS publish took: {}ms, published at: {}",
        (Utc::now() - start).num_milliseconds(),
        Utc::now()
    );

    Ok(())
}

async fn watch_for_cancellation(
    watch: &mut async_nats::jetstream::kv::Watch,
    current_execution_id: i64,
) {
    loop {
        while let Some(Ok(entry)) = watch.next().await {
            let Ok(stored_id) = String::from_utf8(entry.value.to_vec()) else {
                error!("Failed to parse execution ID from watch");
                return;
            };

            println!("stored id {stored_id} current execution id {current_execution_id}");

            if stored_id != current_execution_id.to_string() {
                println!("cancelling stuff");
                return;
            }
        }
    }
}
