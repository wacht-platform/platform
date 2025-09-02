use crate::{AgentExecutor, ResumeContext};
use common::error::AppError;
use common::state::AppState;
use dto::json::StreamEvent;
use futures::StreamExt;
use models::{AiAgentWithFeatures, ExecutionContextStatus};
use queries::{GetExecutionContextQuery, Query};
use tracing::{error, warn};

pub struct AgentHandler {
    app_state: AppState,
}

#[derive(Clone)]
pub struct ExecutionRequest {
    pub agent: AiAgentWithFeatures,
    pub user_message: Option<String>,
    pub user_images: Option<Vec<dto::json::agent_executor::ImageData>>,
    pub context_id: i64,
    pub platform_function_result: Option<(String, serde_json::Value)>,
}

impl AgentHandler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn execute_agent_streaming(&self, request: ExecutionRequest) -> Result<(), AppError> {
        let (sender, receiver) = tokio::sync::mpsc::channel::<StreamEvent>(100);
        let execution_id = self.app_state.sf.next_id()? as i64;
        let context_key = request.context_id.to_string();
        let deployment_id = request.agent.deployment_id;

        let mut executor = AgentExecutor::new(
            request.agent,
            request.context_id,
            self.app_state.clone(),
            sender,
        )
        .await?;

        let kv = self.get_key_value_store().await?;
        let watch = self.create_watcher(&kv, &context_key).await?;
        self.spawn_message_publisher(receiver, context_key.clone(), deployment_id);

        let context = GetExecutionContextQuery::new(request.context_id, deployment_id)
            .execute(&self.app_state)
            .await?;

        let execution_result = match (
            request.user_message,
            request.user_images,
            request.platform_function_result,
            context.status,
        ) {
            (_, _, Some((exec_id, result)), _) => {
                self.resume_agent_execution(
                    &kv,
                    &context_key,
                    execution_id,
                    &mut executor,
                    watch,
                    ResumeContext::PlatformFunction(exec_id, result),
                )
                .await
            }
            (Some(input), _, None, ExecutionContextStatus::WaitingForInput) => {
                self.resume_agent_execution(
                    &kv,
                    &context_key,
                    execution_id,
                    &mut executor,
                    watch,
                    ResumeContext::UserInput(input),
                )
                .await
            }
            (Some(message), images, None, _) => {
                self.run_agent_execution(
                    &kv,
                    &context_key,
                    execution_id,
                    &mut executor,
                    &message,
                    images,
                    watch,
                )
                .await
            }
            _ => Err(AppError::Internal("Invalid execution request".to_string())),
        };

        executor.post_execution_processing();

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
            .map_err(|err| AppError::Internal(format!("Failed to get key-value store: {err}")))
    }

    async fn create_watcher(
        &self,
        kv: &async_nats::jetstream::kv::Store,
        context_key: &str,
    ) -> Result<async_nats::jetstream::kv::Watch, AppError> {
        kv.watch(context_key)
            .await
            .map_err(|err| AppError::Internal(format!("Failed to create watcher: {err}")))
    }

    fn spawn_message_publisher(
        &self,
        mut receiver: tokio::sync::mpsc::Receiver<StreamEvent>,
        context_key: String,
        deployment_id: i64,
    ) {
        let jetstream = self.app_state.nats_jetstream.clone();
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                let _ =
                    publish_stream_event(&jetstream, &context_key, deployment_id, message).await;
            }
        });
    }

    async fn run_agent_execution(
        &self,
        kv: &async_nats::jetstream::kv::Store,
        context_key: &str,
        execution_id: i64,
        agent_executor: &mut AgentExecutor,
        user_message: &str,
        user_images: Option<Vec<dto::json::agent_executor::ImageData>>,
        mut watch: async_nats::jetstream::kv::Watch,
    ) -> Result<(), AppError> {
        kv.put(context_key, execution_id.to_string().into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to store execution ID: {e}")))?;

        tokio::select! {
            result = agent_executor.execute_with_streaming(user_message.to_string(), user_images) => {
                result
            }
            _ = watch_for_cancellation(&mut watch, execution_id) => {
                warn!("Execution cancelled for context {}", context_key);
                Ok(())
            }
        }
    }

    async fn resume_agent_execution(
        &self,
        kv: &async_nats::jetstream::kv::Store,
        context_key: &str,
        execution_id: i64,
        agent_executor: &mut AgentExecutor,
        mut watch: async_nats::jetstream::kv::Watch,
        resume_context: ResumeContext,
    ) -> Result<(), AppError> {
        kv.put(context_key, execution_id.to_string().into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to store execution ID: {e}")))?;

        tokio::select! {
            result = agent_executor.resume_execution(resume_context) => {
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
    deployment_id: i64,
    event: StreamEvent,
) -> Result<(), AppError> {
    let subject = format!("agent_execution_stream.context:{context_key}");

    let (message_type, payload) = match event {
        StreamEvent::ConversationMessage(conversation_content) => {
            let payload = serde_json::to_vec(&conversation_content)
                .map_err(|e| AppError::Internal(format!("Failed to serialize message: {e}")))?;
            ("conversation_message", payload)
        }
        StreamEvent::PlatformEvent(event_label, event_data) => {
            let event_payload = dto::json::PlatformEventPayload {
                event_label,
                event_data,
            };
            let payload = serde_json::to_vec(&event_payload).map_err(|e| {
                AppError::Internal(format!("Failed to serialize platform event: {e}"))
            })?;
            ("platform_event", payload)
        }
        StreamEvent::PlatformFunction(function_name, function_data) => {
            let function_payload = dto::json::PlatformFunctionPayload {
                function_name,
                function_data,
            };
            let payload = serde_json::to_vec(&function_payload).map_err(|e| {
                AppError::Internal(format!("Failed to serialize platform function: {e}"))
            })?;
            ("platform_function", payload)
        }
        StreamEvent::UserInputRequest(user_input_content) => {
            let payload = serde_json::to_vec(&user_input_content).map_err(|e| {
                AppError::Internal(format!("Failed to serialize user input request: {e}"))
            })?;
            ("user_input_request", payload)
        }
    };

    let mut headers = async_nats::HeaderMap::new();
    headers.insert("message_type", message_type);
    headers.insert("context_id", context_key);
    headers.insert("deployment_id", deployment_id.to_string().as_str());

    jetstream
        .publish_with_headers(subject, headers, payload.clone().into())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {e}")))?;

    let worker_task = dto::json::NatsTaskMessage {
        task_id: format!(
            "agent_stream_{}_{}",
            context_key,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ),
        task_type: "agent.stream_log".to_string(),
        payload: serde_json::json!({
            "context_id": context_key,
            "deployment_id": deployment_id.to_string(),
            "message_type": message_type,
            "payload": serde_json::from_slice::<serde_json::Value>(&payload).unwrap_or(serde_json::Value::Null),
        }),
    };

    let worker_payload = serde_json::to_vec(&worker_task)
        .map_err(|e| AppError::Internal(format!("Failed to serialize worker task: {e}")))?;

    jetstream
        .publish("worker.tasks.agent.stream_log", worker_payload.into())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to publish worker task: {e}")))?;

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

            if stored_id != current_execution_id.to_string() {
                return;
            }
        }
    }
}
