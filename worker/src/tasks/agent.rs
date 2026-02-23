use commands::{Command, TriggerWebhookEventCommand};
use common::state::AppState;
use dto::json::{AgentExecutionRequest, AgentExecutionType, AgentStreamMessageType};
use redis::Script;
use tokio::time::{Duration, sleep};

const MAX_DEPLOYMENT_CONCURRENT_EXECUTIONS: i64 = 2000;
const EXECUTION_SLOT_TTL_SECONDS: i64 = 600;
const IDEMPOTENCY_TTL_SECONDS: i64 = 600;
const CONTEXT_LOCK_TTL_SECONDS: i64 = 3600;

#[derive(Debug, Clone)]
enum AgentResolutionStrategy {
    AgentId(i64),
    AgentName(String),
}

impl AgentResolutionStrategy {
    fn display_label(&self) -> String {
        match self {
            Self::AgentId(agent_id) => agent_id.to_string(),
            Self::AgentName(agent_name) => agent_name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
enum AgentExecutionKind {
    NewMessage {
        conversation_id: i64,
    },
    UserInputResponse {
        conversation_id: i64,
    },
    PlatformFunctionResult {
        execution_id: String,
        result: serde_json::Value,
    },
}

#[derive(Debug, Clone)]
struct AgentExecutionEnvelope {
    deployment_id: i64,
    context_id: i64,
    agent_resolution: AgentResolutionStrategy,
    execution_kind: AgentExecutionKind,
}

impl TryFrom<AgentExecutionRequest> for AgentExecutionEnvelope {
    type Error = anyhow::Error;

    fn try_from(request: AgentExecutionRequest) -> Result<Self, Self::Error> {
        let deployment_id = parse_string_id("deployment_id", &request.deployment_id)?;
        let context_id = parse_string_id("context_id", &request.context_id)?;

        let agent_resolution = match (request.agent_id, request.agent_name) {
            (Some(agent_id), _) => {
                AgentResolutionStrategy::AgentId(parse_string_id("agent_id", &agent_id)?)
            }
            (None, Some(agent_name)) => AgentResolutionStrategy::AgentName(agent_name),
            (None, None) => {
                return Err(anyhow::anyhow!(
                    "Either agent_id or agent_name must be provided"
                ));
            }
        };

        let execution_kind = match request.execution_type {
            AgentExecutionType::NewMessage { conversation_id } => AgentExecutionKind::NewMessage {
                conversation_id: parse_string_id("conversation_id", &conversation_id)?,
            },
            AgentExecutionType::UserInputResponse { conversation_id } => {
                AgentExecutionKind::UserInputResponse {
                    conversation_id: parse_string_id("conversation_id", &conversation_id)?,
                }
            }
            AgentExecutionType::PlatformFunctionResult {
                execution_id,
                result,
            } => AgentExecutionKind::PlatformFunctionResult {
                execution_id,
                result,
            },
        };

        Ok(Self {
            deployment_id,
            context_id,
            agent_resolution,
            execution_kind,
        })
    }
}

fn parse_string_id(field_name: &str, raw_value: &str) -> Result<i64, anyhow::Error> {
    raw_value
        .parse::<i64>()
        .map_err(|error| anyhow::anyhow!("Invalid {} '{}': {}", field_name, raw_value, error))
}

fn console_deployment_id() -> i64 {
    std::env::var("CONSOLE_DEPLOYMENT_ID")
        .unwrap_or_else(|_| "0".to_string())
        .parse()
        .unwrap_or(0)
}

async fn trigger_execution_webhook(
    app_state: &AppState,
    deployment_id: i64,
    event_name: &str,
    payload: serde_json::Value,
    error_context: &str,
) {
    let trigger_command = TriggerWebhookEventCommand::new(
        console_deployment_id(),
        deployment_id.to_string(),
        event_name.to_string(),
        payload,
    );

    if let Err(error) = trigger_command.execute(app_state).await {
        tracing::error!("Failed to trigger {} webhook: {}", error_context, error);
    }
}

async fn publish_conversation_webhook(
    app_state: &AppState,
    deployment_id: i64,
    context_id: i64,
    conversation_id: i64,
    message_type: &str,
    error_context: &str,
) {
    use queries::Query;

    if let Ok(conversation) = queries::GetConversationByIdQuery::new(conversation_id)
        .execute(app_state)
        .await
    {
        let payload = serde_json::json!({
            "context_id": context_id,
            "message_type": message_type,
            "data": conversation.content,
            "timestamp": conversation.timestamp,
        });

        trigger_execution_webhook(
            app_state,
            deployment_id,
            "execution_context.message",
            payload,
            error_context,
        )
        .await;
    }
}

pub async fn process_agent_execution(
    app_state: &AppState,
    request: AgentExecutionRequest,
) -> Result<String, anyhow::Error> {
    use agent_engine::{AgentHandler, ExecutionRequest};
    use queries::{
        GetAiAgentByIdWithFeatures, GetAiAgentByNameWithFeatures, GetExecutionContextQuery, Query,
    };

    let execution_envelope = AgentExecutionEnvelope::try_from(request)?;
    if !register_execution_idempotency(app_state, &execution_envelope).await? {
        return Ok(format!(
            "Duplicate execution ignored for context {}",
            execution_envelope.context_id
        ));
    }

    let concurrency_guard =
        acquire_deployment_execution_slot(app_state, execution_envelope.deployment_id).await?;
    let context_guard =
        acquire_context_execution_lock(app_state, execution_envelope.context_id).await?;

    let agent_identifier = execution_envelope.agent_resolution.display_label();

    tracing::info!(
        "Processing agent '{}' execution for context {} (type: {:?})",
        agent_identifier,
        execution_envelope.context_id,
        execution_envelope.execution_kind
    );

    let agent = match &execution_envelope.agent_resolution {
        AgentResolutionStrategy::AgentId(agent_id) => GetAiAgentByIdWithFeatures::new(*agent_id)
            .execute(app_state)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get agent by ID {}: {}", agent_id, e))?,
        AgentResolutionStrategy::AgentName(agent_name) => {
            GetAiAgentByNameWithFeatures::new(execution_envelope.deployment_id, agent_name.clone())
                .execute(app_state)
                .await
                .map_err(|_| anyhow::anyhow!("Agent '{}' not found", agent_name))?
        }
    };

    let deployment_id = execution_envelope.deployment_id;
    let context_id = execution_envelope.context_id;
    let execution_kind = execution_envelope.execution_kind;
    let context = GetExecutionContextQuery::new(context_id, deployment_id)
        .execute(app_state)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load execution context {}: {}", context_id, e))?;
    if matches!(
        context.status,
        models::ExecutionContextStatus::Failed
            | models::ExecutionContextStatus::Interrupted
            | models::ExecutionContextStatus::Completed
    ) {
        drop(context_guard);
        drop(concurrency_guard);
        return Ok(format!(
            "Skipped execution for context {} because status is {}",
            context_id, context.status
        ));
    }

    let execution_request = match execution_kind {
        AgentExecutionKind::NewMessage { conversation_id } => {
            let conv_id = conversation_id;
            tracing::info!("New message execution with conversation_id: {}", conv_id);

            publish_conversation_webhook(
                app_state,
                deployment_id,
                context_id,
                conv_id,
                AgentStreamMessageType::ConversationMessage.as_header_value(),
                "user message",
            )
            .await;

            ExecutionRequest {
                agent,
                conversation_id: Some(conv_id),
                context_id,
                platform_function_result: None,
            }
        }
        AgentExecutionKind::UserInputResponse { conversation_id } => {
            let conv_id = conversation_id;
            tracing::info!("User input response with conversation_id: {}", conv_id);

            publish_conversation_webhook(
                app_state,
                deployment_id,
                context_id,
                conv_id,
                "user_input_response",
                "user response",
            )
            .await;

            ExecutionRequest {
                agent,
                conversation_id: Some(conv_id),
                context_id,
                platform_function_result: None,
            }
        }
        AgentExecutionKind::PlatformFunctionResult {
            execution_id,
            result,
        } => {
            tracing::info!(
                "Platform function result for execution_id: {}",
                execution_id
            );

            let webhook_payload = serde_json::json!({
                "context_id": context_id,
                "message_type": AgentStreamMessageType::PlatformFunction.as_header_value(),
                "execution_id": execution_id,
                "data": result,
                "timestamp": chrono::Utc::now(),
            });

            trigger_execution_webhook(
                app_state,
                deployment_id,
                "execution_context.platform_function_result",
                webhook_payload,
                "platform function result",
            )
            .await;

            ExecutionRequest {
                agent,
                conversation_id: None,
                context_id,
                platform_function_result: Some((execution_id, result)),
            }
        }
    };

    let result = AgentHandler::new(app_state.clone())
        .execute_agent_streaming(execution_request)
        .await;

    drop(context_guard);
    drop(concurrency_guard);
    result.map_err(|e| anyhow::anyhow!("Agent execution failed: {}", e))?;

    Ok(format!(
        "Agent '{}' execution completed for context {}",
        agent_identifier, context_id
    ))
}

struct DeploymentExecutionGuard {
    app_state: AppState,
    key: String,
}

impl Drop for DeploymentExecutionGuard {
    fn drop(&mut self) {
        let app_state = self.app_state.clone();
        let key = self.key.clone();
        tokio::spawn(async move {
            if let Ok(mut conn) = app_state
                .redis_client
                .get_multiplexed_async_connection()
                .await
            {
                let _: Result<i64, _> = redis::cmd("DECR").arg(&key).query_async(&mut conn).await;
            }
        });
    }
}

struct ContextExecutionLockGuard {
    app_state: AppState,
    key: String,
    token: String,
}

impl Drop for ContextExecutionLockGuard {
    fn drop(&mut self) {
        let app_state = self.app_state.clone();
        let key = self.key.clone();
        let token = self.token.clone();
        tokio::spawn(async move {
            if let Ok(mut conn) = app_state
                .redis_client
                .get_multiplexed_async_connection()
                .await
            {
                let unlock_script = Script::new(
                    r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('DEL', KEYS[1])
end
return 0
"#,
                );
                let _: Result<i64, _> = unlock_script
                    .key(&key)
                    .arg(&token)
                    .invoke_async(&mut conn)
                    .await;
            }
        });
    }
}

async fn acquire_deployment_execution_slot(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentExecutionGuard, anyhow::Error> {
    let key = format!("agent:deployment_active_executions:{}", deployment_id);
    let script = Script::new(
        r#"
local key = KEYS[1]
local max_active = tonumber(ARGV[1])
local ttl_sec = tonumber(ARGV[2])
local current = tonumber(redis.call('GET', key) or '0')
if current >= max_active then
  return 0
end
current = redis.call('INCR', key)
redis.call('EXPIRE', key, ttl_sec)
return current
"#,
    );

    loop {
        let mut conn = app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await?;
        let acquired_count: i64 = script
            .key(&key)
            .arg(MAX_DEPLOYMENT_CONCURRENT_EXECUTIONS)
            .arg(EXECUTION_SLOT_TTL_SECONDS)
            .invoke_async(&mut conn)
            .await?;
        if acquired_count > 0 {
            return Ok(DeploymentExecutionGuard {
                app_state: app_state.clone(),
                key,
            });
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn acquire_context_execution_lock(
    app_state: &AppState,
    context_id: i64,
) -> Result<ContextExecutionLockGuard, anyhow::Error> {
    let key = format!("agent:context_execution_lock:{}", context_id);
    let token = format!(
        "{}:{}",
        context_id,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let script = Script::new(
        r#"
if redis.call('SET', KEYS[1], ARGV[1], 'NX', 'EX', ARGV[2]) then
  return 1
end
return 0
"#,
    );

    loop {
        let mut conn = app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await?;
        let acquired: i64 = script
            .key(&key)
            .arg(&token)
            .arg(CONTEXT_LOCK_TTL_SECONDS)
            .invoke_async(&mut conn)
            .await?;
        if acquired == 1 {
            return Ok(ContextExecutionLockGuard {
                app_state: app_state.clone(),
                key,
                token,
            });
        }
        sleep(Duration::from_millis(100)).await;
    }
}

async fn register_execution_idempotency(
    app_state: &AppState,
    envelope: &AgentExecutionEnvelope,
) -> Result<bool, anyhow::Error> {
    let identity = match &envelope.execution_kind {
        AgentExecutionKind::NewMessage { conversation_id } => format!("new:{}", conversation_id),
        AgentExecutionKind::UserInputResponse { conversation_id } => {
            format!("input:{}", conversation_id)
        }
        AgentExecutionKind::PlatformFunctionResult { execution_id, .. } => {
            format!("platform:{}", execution_id)
        }
    };

    let key = format!(
        "agent:exec_idempotency:{}:{}:{}",
        envelope.deployment_id, envelope.context_id, identity
    );

    let script = Script::new(
        r#"
if redis.call('SET', KEYS[1], ARGV[1], 'NX', 'EX', ARGV[2]) then
  return 1
end
return 0
"#,
    );

    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;
    let inserted: i64 = script
        .key(key)
        .arg("1")
        .arg(IDEMPOTENCY_TTL_SECONDS)
        .invoke_async(&mut conn)
        .await?;
    Ok(inserted == 1)
}
