use crate::{AgentExecutor, ResumeContext};
use commands::Command;
use common::error::AppError;
use common::state::AppState;
use dto::json::StreamEvent;
use futures::StreamExt;
use models::AiAgentWithFeatures;
use models::ExecutionContextStatus;
use queries::Query;
use serde_json::Value;
use tracing::{error, warn};

pub struct AgentHandler {
    app_state: AppState,
}

enum SpawnControlSignal {
    Stop,
    Restart,
    UpdateParams(Value),
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

        let kv = self.get_key_value_store().await?;
        let watch = self.create_watcher(&kv, &context_key).await?;

        let deployment_ai_settings = queries::GetDeploymentAiSettingsQuery::new(deployment_id)
            .execute(&self.app_state)
            .await
            .ok()
            .flatten();

        let execution_context = crate::execution_context::ExecutionContext::new(
            self.app_state.clone(),
            request.agent.clone(),
            request.context_id,
            deployment_ai_settings,
        );

        self.spawn_message_publisher(receiver, context_key.clone(), execution_context.clone());

        let app_state = self.app_state.clone();
        let child_context_id = request.context_id;

        let context = execution_context.get_context().await?;

        let execution_context_for_notification = execution_context.clone();
        let spawn_control_sub = if context.parent_context_id.is_some() {
            subscribe_spawn_control(&app_state, child_context_id).await
        } else {
            None
        };

        let mut executor = AgentExecutor::new(execution_context, sender).await?;
        let mut spawn_control_sub = spawn_control_sub;

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
                    context.parent_context_id,
                    context.id,
                    deployment_id,
                    &mut spawn_control_sub,
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
                    context.parent_context_id,
                    context.id,
                    deployment_id,
                    &mut spawn_control_sub,
                )
                .await
            }
            _ => Err(AppError::Internal(
                "Invalid execution request: conversation_id required".to_string(),
            )),
        };

        if let Err(error) = &execution_result {
            let _ = commands::UpdateExecutionContextQuery::new(context.id, deployment_id)
                .with_status(models::ExecutionContextStatus::Failed)
                .execute(&self.app_state)
                .await;

            let _ = commands::StoreCompletionSummaryEnhancedCommand::new(
                context.id,
                deployment_id,
                commands::CompletionSummary {
                    status: commands::CompletionStatus::Failed,
                    result: None,
                    error_message: Some(format!("Execution failed: {}", error)),
                    metrics: None,
                },
            )
            .execute(&self.app_state)
            .await;
        }

        executor.post_execution_processing();

        execution_context_for_notification.invalidate_cache();
        if let Ok(context) = execution_context_for_notification.get_context().await {
            if let Some(parent_id) = context.parent_context_id {
                let current_status = match context.status {
                    models::ExecutionContextStatus::Completed => "completed",
                    models::ExecutionContextStatus::Failed => "failed",
                    _ => return Ok(()),
                };

                if current_status == "completed" || current_status == "failed" {
                    let completion_event = dto::json::StreamEvent::ChildAgentCompleted {
                        child_context_id: context.id,
                        status: current_status.to_string(),
                        summary: context.completion_summary,
                    };

                    if let Err(e) = publish_stream_event(
                        execution_context_for_notification.clone(),
                        &parent_id.to_string(),
                        completion_event,
                    )
                    .await
                    {
                        warn!(
                            "Failed to publish child completion to parent {}: {}",
                            parent_id, e
                        );
                    } else {
                        warn!(
                            "Notified parent {} that child {} completed (status: {})",
                            parent_id, context.id, current_status
                        );
                    }
                }
            }
        }

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
        ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
    ) {
        tokio::spawn(async move {
            while let Some(message) = receiver.recv().await {
                let _ = publish_stream_event(ctx.clone(), &context_key, message).await;
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
        parent_context_id: Option<i64>,
        context_id: i64,
        deployment_id: i64,
        spawn_control_sub: &mut Option<async_nats::Subscriber>,
    ) -> Result<(), AppError> {
        kv.put(context_key, execution_id.to_string().into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to store execution ID: {e}")))?;

        if let Some(parent_id) = parent_context_id {
            loop {
                tokio::select! {
                    result = agent_executor.execute_with_conversation_id(conversation_id) => {
                        return result;
                    }
                    _ = watch_for_cancellation(&mut watch, execution_id) => {
                        warn!("Execution cancelled for context {}", context_key);
                        mark_context_cancelled(
                            &self.app_state,
                            context_id,
                            deployment_id,
                        ).await;
                        return Ok(());
                    }
                    signal = wait_for_spawn_control(spawn_control_sub), if spawn_control_sub.is_some() => {
                        match signal {
                            SpawnControlSignal::Stop => {
                                warn!("Spawn control stop received from parent context {}", parent_id);
                                mark_context_failed_due_to_parent_abort(
                                    &self.app_state,
                                    context_id,
                                    deployment_id,
                                    parent_id,
                                ).await?;
                                return Ok(());
                            }
                            SpawnControlSignal::Restart => {
                                warn!("Spawn control restart received for child context {}", context_id);
                                record_spawn_control_restart(
                                    &self.app_state,
                                    context_id,
                                    deployment_id,
                                ).await?;
                                continue;
                            }
                            SpawnControlSignal::UpdateParams(params) => {
                                warn!("Spawn control update_params received for child context {}", context_id);
                                apply_spawn_control_params(
                                    &self.app_state,
                                    context_id,
                                    deployment_id,
                                    params,
                                ).await?;
                                continue;
                            }
                        }
                    }
                }
            }
        } else {
            tokio::select! {
                result = agent_executor.execute_with_conversation_id(conversation_id) => {
                    result
                }
                _ = watch_for_cancellation(&mut watch, execution_id) => {
                    warn!("Execution cancelled for context {}", context_key);
                    mark_context_cancelled(
                        &self.app_state,
                        context_id,
                        deployment_id,
                    ).await;
                    Ok(())
                }
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
        parent_context_id: Option<i64>,
        context_id: i64,
        deployment_id: i64,
        spawn_control_sub: &mut Option<async_nats::Subscriber>,
    ) -> Result<(), AppError> {
        kv.put(context_key, execution_id.to_string().into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to store execution ID: {e}")))?;

        if let Some(parent_id) = parent_context_id {
            loop {
                tokio::select! {
                    result = agent_executor.resume_execution(resume_context.clone()) => {
                        return result;
                    }
                    _ = watch_for_cancellation(&mut watch, execution_id) => {
                        warn!("Execution cancelled for context {}", context_key);
                        mark_context_cancelled(
                            &self.app_state,
                            context_id,
                            deployment_id,
                        ).await;
                        return Ok(());
                    }
                    signal = wait_for_spawn_control(spawn_control_sub), if spawn_control_sub.is_some() => {
                        match signal {
                            SpawnControlSignal::Stop => {
                                warn!("Parent context {} aborted; cancelling child context {}", parent_id, context_key);
                                mark_context_failed_due_to_parent_abort(
                                    &self.app_state,
                                    context_id,
                                    deployment_id,
                                    parent_id,
                                ).await?;
                                return Ok(());
                            }
                            SpawnControlSignal::Restart => {
                                warn!("Spawn control restart received for child context {}", context_id);
                                record_spawn_control_restart(
                                    &self.app_state,
                                    context_id,
                                    deployment_id,
                                ).await?;
                                continue;
                            }
                            SpawnControlSignal::UpdateParams(params) => {
                                warn!("Spawn control update_params received for child context {}", context_id);
                                apply_spawn_control_params(
                                    &self.app_state,
                                    context_id,
                                    deployment_id,
                                    params,
                                ).await?;
                                continue;
                            }
                        }
                    }
                }
            }
        } else {
            tokio::select! {
                result = agent_executor.resume_execution(resume_context) => {
                    result
                }
                _ = watch_for_cancellation(&mut watch, execution_id) => {
                    warn!("Execution cancelled for context {}", context_key);
                    mark_context_cancelled(
                        &self.app_state,
                        context_id,
                        deployment_id,
                    ).await;
                    Ok(())
                }
            }
        }
    }
}

async fn publish_stream_event(
    ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
    context_key: &str,
    event: StreamEvent,
) -> Result<(), AppError> {
    let app_state = &ctx.app_state;
    let deployment_id = ctx.agent.deployment_id;
    let jetstream = &app_state.nats_jetstream;
    let subject = format!("agent_execution_stream.context:{context_key}");

    let (message_type, payload) = dto::json::encode_stream_event(&event)
        .map_err(|e| AppError::Internal(format!("Failed to encode stream event: {e}")))?;

    let mut headers = async_nats::HeaderMap::new();
    headers.insert("message_type", message_type.as_header_value());
    headers.insert("context_id", context_key);
    headers.insert("deployment_id", deployment_id.to_string().as_str());

    tracing::info!(
        "Publishing {} to subject {} for context {}",
        message_type.as_header_value(),
        subject,
        context_key
    );

    jetstream
        .publish_with_headers(subject.clone(), headers, payload.clone().into())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {e}")))?;

    tracing::info!(
        "Successfully published {} to {}",
        message_type.as_header_value(),
        subject
    );

    use commands::{Command, TriggerWebhookEventCommand};

    let webhook_payload = serde_json::json!({
        "context_id": context_key,
        "message_type": message_type.as_header_value(),
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
        message_type.webhook_event_name().to_string(),
        webhook_payload,
    );

    if let Err(e) = trigger_command.execute(app_state).await {
        tracing::warn!(
            deployment_id = deployment_id,
            webhook_event = %message_type.webhook_event_name(),
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

async fn mark_context_failed_due_to_parent_abort(
    app_state: &AppState,
    context_id: i64,
    deployment_id: i64,
    parent_context_id: i64,
) -> Result<(), AppError> {
    commands::UpdateExecutionContextQuery::new(context_id, deployment_id)
        .with_status(ExecutionContextStatus::Failed)
        .execute(app_state)
        .await?;

    commands::StoreCompletionSummaryEnhancedCommand::new(
        context_id,
        deployment_id,
        commands::CompletionSummary {
            status: commands::CompletionStatus::Cancelled,
            result: None,
            error_message: Some(format!(
                "Execution cancelled because parent context {} was aborted.",
                parent_context_id
            )),
            metrics: None,
        },
    )
    .execute(app_state)
    .await?;

    Ok(())
}

/// Marks a context as cancelled when the user cancels execution via KV watcher.
/// Setting status to Failed triggers CancelDescendantExecutionsCommand internally,
/// which BFS-walks all descendants and sends NATS stop signals.
async fn mark_context_cancelled(app_state: &AppState, context_id: i64, deployment_id: i64) {
    let _ = commands::UpdateExecutionContextQuery::new(context_id, deployment_id)
        .with_status(ExecutionContextStatus::Failed)
        .execute(app_state)
        .await;

    let _ = commands::StoreCompletionSummaryEnhancedCommand::new(
        context_id,
        deployment_id,
        commands::CompletionSummary {
            status: commands::CompletionStatus::Cancelled,
            result: None,
            error_message: Some("Execution cancelled by user.".to_string()),
            metrics: None,
        },
    )
    .execute(app_state)
    .await;
}

async fn subscribe_spawn_control(
    app_state: &AppState,
    context_id: i64,
) -> Option<async_nats::Subscriber> {
    let subject = format!("agent_spawn_control.context:{}", context_id);
    match app_state.nats_client.subscribe(subject).await {
        Ok(subscriber) => Some(subscriber),
        Err(error) => {
            warn!(
                "Failed to subscribe to spawn control for context {}: {}",
                context_id, error
            );
            None
        }
    }
}

async fn wait_for_spawn_control(
    subscriber: &mut Option<async_nats::Subscriber>,
) -> SpawnControlSignal {
    let Some(subscriber) = subscriber.as_mut() else {
        return SpawnControlSignal::Stop;
    };

    while let Some(message) = subscriber.next().await {
        let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&message.payload) else {
            continue;
        };
        let action = payload
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if action.eq_ignore_ascii_case("stop") {
            return SpawnControlSignal::Stop;
        }
        if action.eq_ignore_ascii_case("restart") {
            return SpawnControlSignal::Restart;
        }
        if action.eq_ignore_ascii_case("update_params") {
            let value = payload.get("value").cloned().unwrap_or(serde_json::json!({}));
            return SpawnControlSignal::UpdateParams(value);
        }
    }

    SpawnControlSignal::Stop
}

async fn apply_spawn_control_params(
    app_state: &AppState,
    context_id: i64,
    deployment_id: i64,
    params: Value,
) -> Result<(), AppError> {
    let context = queries::GetExecutionContextQuery::new(context_id, deployment_id)
        .execute(app_state)
        .await?;

    let mut metadata = context.external_resource_metadata.unwrap_or_else(|| serde_json::json!({}));
    if !metadata.is_object() {
        metadata = serde_json::json!({});
    }
    if let Some(obj) = metadata.as_object_mut() {
        obj.insert("spawn_control_params".to_string(), params);
        obj.insert(
            "spawn_control_params_updated_at".to_string(),
            serde_json::json!(chrono::Utc::now().to_rfc3339()),
        );
    }

    commands::UpdateExecutionContextQuery::new(context_id, deployment_id)
        .with_external_resource_metadata(metadata)
        .execute(app_state)
        .await?;

    commands::PostStatusUpdateCommand::new(
        context_id,
        deployment_id,
        "Parent updated execution parameters".to_string(),
    )
    .execute(app_state)
    .await?;

    Ok(())
}

async fn record_spawn_control_restart(
    app_state: &AppState,
    context_id: i64,
    deployment_id: i64,
) -> Result<(), AppError> {
    commands::PostStatusUpdateCommand::new(
        context_id,
        deployment_id,
        "Parent requested execution restart".to_string(),
    )
    .execute(app_state)
    .await?;
    Ok(())
}
