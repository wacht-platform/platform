use crate::{AgentExecutor, ResumeContext, teams_logger::TeamsActivityLogger};
use common::error::AppError;
use common::state::AppState;
use dto::json::StreamEvent;
use futures::StreamExt;
use models::AiAgentWithFeatures;
use queries::{GetExecutionContextQuery, Query};
use tracing::{error, warn};

pub struct AgentHandler {
    app_state: AppState,
}

#[derive(Clone)]
pub struct ExecutionRequest {
    pub agent: AiAgentWithFeatures,
    pub conversation_id: Option<i64>,
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
        let agent_id = request.agent.id;

        let kv = self.get_key_value_store().await?;
        let watch = self.create_watcher(&kv, &context_key).await?;
        self.spawn_message_publisher(receiver, context_key.clone(), deployment_id, agent_id);

        let context = GetExecutionContextQuery::new(request.context_id, deployment_id)
            .execute(&self.app_state)
            .await?;
        
        // Extract title ensuring we have a reasonable default
        let context_title = if context.title.is_empty() {
            format!("Context {}", request.context_id)
        } else {
            context.title.clone()
        };

        let mut executor = AgentExecutor::new(
            request.agent,
            request.context_id,
            self.app_state.clone(),
            sender,
            context_title,
        )
        .await?;

        let execution_result = match (
            request.conversation_id,
            request.platform_function_result,
            context.status,
        ) {
            (_, Some((exec_id, result)), _) => {
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
            (Some(conv_id), None, _) => {
                self.run_agent_execution(
                    &kv,
                    &context_key,
                    execution_id,
                    &mut executor,
                    conv_id,
                    watch,
                )
                .await
            }
            _ => Err(AppError::Internal("Invalid execution request: conversation_id required".to_string())),
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
        agent_id: i64,
    ) {
        let app_state = self.app_state.clone();
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                let _ =
                    publish_stream_event(&app_state, &context_key, deployment_id, agent_id, message).await;
            }
        });
    }

    async fn run_agent_execution(
        &self,
        kv: &async_nats::jetstream::kv::Store,
        context_key: &str,
        execution_id: i64,
        agent_executor: &mut AgentExecutor,
        conversation_id: i64,
        mut watch: async_nats::jetstream::kv::Watch,
    ) -> Result<(), AppError> {
        kv.put(context_key, execution_id.to_string().into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to store execution ID: {e}")))?;

        tokio::select! {
            result = agent_executor.execute_with_conversation_id(conversation_id) => {
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
    app_state: &AppState,
    context_key: &str,
    deployment_id: i64,
    agent_id: i64,
    event: StreamEvent,
) -> Result<(), AppError> {
    let jetstream = &app_state.nats_jetstream;
    let subject = format!("agent_execution_stream.context:{context_key}");

    let (message_type, payload) = match &event {
        StreamEvent::ConversationMessage(conversation_content) => {
            // Log outgoing agent response if from Teams context
            if let models::ConversationContent::AgentResponse { response, .. } = &conversation_content.content {
                if let Ok(ctx_id) = context_key.parse::<i64>() {
                    if let Ok(ctx) = GetExecutionContextQuery::new(ctx_id, deployment_id)
                        .execute(app_state)
                        .await 
                    {
                        if ctx.source.as_deref() == Some("teams") {
                            if let Some(group) = &ctx.context_group {
                                if !group.is_empty() {
                                    let mut location = String::new();
                                    if let Some(meta) = &ctx.external_resource_metadata {
                                        if let Some(channel_name) = meta.get("channelName").and_then(|v| v.as_str()) {
                                            location = format!(" [Channel: {}]", channel_name);
                                        }
                                    }
                                    
                                    let title = if ctx.title.is_empty() {
                                        format!("Context {}", ctx.id) 
                                    } else { 
                                        ctx.title.clone() 
                                    };
                                    let logger = TeamsActivityLogger::new(&deployment_id.to_string(), &agent_id.to_string(), group, &title);
                                    let _ = logger.append_entry("RESPONSE", &format!("To User{}: {}", location, response)).await;
                                }
                            }
                        }
                    }
                }
            }

            let payload = serde_json::to_vec(&conversation_content)
                .map_err(|e| AppError::Internal(format!("Failed to serialize message: {e}")))?;
            ("conversation_message", payload)
        }
        StreamEvent::PlatformEvent(event_label, event_data) => {
            let event_payload = dto::json::PlatformEventPayload {
                event_label: event_label.clone(),
                event_data: event_data.clone(),
            };
            let payload = serde_json::to_vec(&event_payload).map_err(|e| {
                AppError::Internal(format!("Failed to serialize platform event: {e}"))
            })?;
            ("platform_event", payload)
        }
        StreamEvent::PlatformFunction(function_name, function_data) => {
            let function_payload = dto::json::PlatformFunctionPayload {
                function_name: function_name.clone(),
                function_data: function_data.clone(),
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

    // 1. Publish to Realtime Stream
    jetstream
        .publish_with_headers(subject, headers, payload.clone().into())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {e}")))?;

    // 2. Trigger Webhook Directly (Streamlined)
    use commands::{Command, TriggerWebhookEventCommand};

    let webhook_event = match message_type {
        "conversation_message" => "execution_context.message",
        "platform_event" => "execution_context.platform_event",
        "platform_function" => "execution_context.platform_function",
        "user_input_request" => "execution_context.user_input_request",
        _ => "execution_context.message",
    };

    let webhook_payload = serde_json::json!({
        "context_id": context_key,
        "message_type": message_type,
        "data": serde_json::from_slice::<serde_json::Value>(&payload).unwrap_or(serde_json::Value::Null),
        "timestamp": chrono::Utc::now(),
    });

    let console_id = std::env::var("CONSOLE_DEPLOYMENT_ID")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap_or(0);

    let trigger_command = TriggerWebhookEventCommand::new(
        console_id,
        deployment_id.to_string(),
        webhook_event.to_string(),
        webhook_payload,
    );

    // Run webhook trigger in background or await? 
    // Since publish_stream_event is spawned in a loop, awaiting is fine/good.
    // However, if webhook is slow, it might block stream?
    // publish_stream_event is inside a tokio::spawn loop in spawn_message_publisher
    // Yes, awaiting is correct.
    
    if let Err(e) = trigger_command.execute(app_state).await {
         tracing::warn!(
            deployment_id = deployment_id,
            webhook_event = %webhook_event,
            context_key = %context_key,
            "Failed to trigger webhook for agent stream event: {}. This is expected if no webhook is configured.", 
            e
        );
    }

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
