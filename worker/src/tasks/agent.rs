use agent_engine::{AgentHandler, ExecutionRequest};
use commands::webhook_trigger::TriggerWebhookEventCommand;
use common::state::AppState;
use dto::json::{AgentExecutionRequest, AgentExecutionType, AgentStreamMessageType};
use models::AiAgentWithFeatures;
use queries::{GetAiAgentByIdWithFeatures, GetAiAgentByNameWithFeatures};
use redis::Script;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio::time::{Duration, interval, sleep};

const MAX_DEPLOYMENT_CONCURRENT_EXECUTIONS: i64 = 2000;
const EXECUTION_SLOT_TTL_SECONDS: i64 = 600;
const IDEMPOTENCY_TTL_SECONDS: i64 = 600;
const CONTEXT_LOCK_TTL_SECONDS: i64 = 3600;
const EXECUTION_SLOT_HEARTBEAT_SECONDS: u64 = 120;
const CONTEXT_LOCK_HEARTBEAT_SECONDS: u64 = 300;
const MAX_LOCK_WAIT_SECONDS: u64 = 300; // 5 minutes
const DEPLOYMENT_SLOT_BACKOFF_MIN_MS: u64 = 250;
const DEPLOYMENT_SLOT_BACKOFF_MAX_MS: u64 = 5_000;
const CONTEXT_LOCK_BACKOFF_MIN_MS: u64 = 100;
const CONTEXT_LOCK_BACKOFF_MAX_MS: u64 = 2_000;
const BACKOFF_JITTER_MAX_MS: u64 = 250;
const REDIS_SET_NX_EX_LUA: &str = r#"
if redis.call('SET', KEYS[1], ARGV[1], 'NX', 'EX', ARGV[2]) then
  return 1
end
return 0
"#;

#[derive(Debug)]
pub enum AgentExecutionError {
    ExecutionBusy {
        resource: &'static str,
        identifier: i64,
        max_wait_seconds: u64,
    },
    Other(anyhow::Error),
}

impl std::fmt::Display for AgentExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutionBusy {
                resource,
                identifier,
                max_wait_seconds,
            } => write!(
                f,
                "ExecutionBusy: timed out waiting for {} lock (id={}, max_wait_seconds={})",
                resource, identifier, max_wait_seconds
            ),
            Self::Other(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for AgentExecutionError {}

impl From<anyhow::Error> for AgentExecutionError {
    fn from(value: anyhow::Error) -> Self {
        Self::Other(value)
    }
}

impl From<redis::RedisError> for AgentExecutionError {
    fn from(value: redis::RedisError) -> Self {
        Self::Other(value.into())
    }
}

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
    let trigger_deps = common::deps::from_app(app_state).db().redis().nats().id();
    let trigger_command = TriggerWebhookEventCommand::new(
        console_deployment_id(),
        deployment_id.to_string(),
        event_name.to_string(),
        payload,
    );

    if let Err(error) = trigger_command.execute_with_deps(&trigger_deps).await {
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
    let conversation_query = queries::GetConversationByIdQuery::new(conversation_id);
    if let Ok(conversation) = conversation_query
        .execute_with_db(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
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

async fn build_conversation_execution_request(
    app_state: &AppState,
    deployment_id: i64,
    context_id: i64,
    conversation_id: i64,
    message_type: &str,
    error_context: &str,
    agent: AiAgentWithFeatures,
) -> ExecutionRequest {
    tracing::info!(
        "{} with conversation_id: {}",
        error_context,
        conversation_id
    );

    publish_conversation_webhook(
        app_state,
        deployment_id,
        context_id,
        conversation_id,
        message_type,
        error_context,
    )
    .await;

    ExecutionRequest {
        agent,
        conversation_id: Some(conversation_id),
        context_id,
        platform_function_result: None,
    }
}

async fn load_agent_for_execution(
    app_state: &AppState,
    deployment_id: i64,
    resolution: &AgentResolutionStrategy,
) -> Result<AiAgentWithFeatures, AgentExecutionError> {
    match resolution {
        AgentResolutionStrategy::AgentId(agent_id) => GetAiAgentByIdWithFeatures::new(*agent_id)
            .execute_with_db(
                app_state
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await
            .map_err(|e| {
                AgentExecutionError::Other(anyhow::anyhow!(
                    "Failed to get agent by ID {}: {}",
                    agent_id,
                    e
                ))
            }),
        AgentResolutionStrategy::AgentName(agent_name) => {
            GetAiAgentByNameWithFeatures::new(deployment_id, agent_name.clone())
                .execute_with_db(
                    app_state
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await
                .map_err(|_| {
                    AgentExecutionError::Other(anyhow::anyhow!("Agent '{}' not found", agent_name))
                })
        }
    }
}

pub async fn process_agent_execution(
    app_state: &AppState,
    request: AgentExecutionRequest,
) -> Result<String, AgentExecutionError> {
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

    let agent = load_agent_for_execution(
        app_state,
        execution_envelope.deployment_id,
        &execution_envelope.agent_resolution,
    )
    .await?;

    let deployment_id = execution_envelope.deployment_id;
    let context_id = execution_envelope.context_id;
    let execution_kind = execution_envelope.execution_kind;

    let execution_request = match execution_kind {
        AgentExecutionKind::NewMessage { conversation_id } => build_conversation_execution_request(
            app_state,
            deployment_id,
            context_id,
            conversation_id,
            AgentStreamMessageType::ConversationMessage.as_header_value(),
            "New message execution",
            agent,
        )
        .await,
        AgentExecutionKind::UserInputResponse { conversation_id } => {
            build_conversation_execution_request(
                app_state,
                deployment_id,
                context_id,
                conversation_id,
                "user_input_response",
                "User input response",
                agent,
            )
            .await
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
    result.map_err(|e| {
        AgentExecutionError::Other(anyhow::anyhow!("Agent execution failed: {}", e))
    })?;

    Ok(format!(
        "Agent '{}' execution completed for context {}",
        agent_identifier, context_id
    ))
}

struct DeploymentExecutionGuard {
    app_state: AppState,
    key: String,
    heartbeat_stop: Option<oneshot::Sender<()>>,
}

impl Drop for DeploymentExecutionGuard {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.heartbeat_stop.take() {
            let _ = stop_tx.send(());
        }

        let app_state = self.app_state.clone();
        let key = self.key.clone();
        tokio::spawn(async move {
            if let Ok(mut conn) = app_state
                .redis_client
                .get_multiplexed_async_connection()
                .await
            {
                let decrement_script = Script::new(
                    r#"
local current = tonumber(redis.call('GET', KEYS[1]) or '0')
if current <= 1 then
  return redis.call('DEL', KEYS[1])
end
return redis.call('DECR', KEYS[1])
"#,
                );
                let _: Result<i64, _> = decrement_script.key(&key).invoke_async(&mut conn).await;
            }
        });
    }
}

struct ContextExecutionLockGuard {
    app_state: AppState,
    key: String,
    token: String,
    heartbeat_stop: Option<oneshot::Sender<()>>,
}

impl Drop for ContextExecutionLockGuard {
    fn drop(&mut self) {
        if let Some(stop_tx) = self.heartbeat_stop.take() {
            let _ = stop_tx.send(());
        }

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

fn spawn_deployment_slot_heartbeat(app_state: AppState, key: String) -> oneshot::Sender<()> {
    let (stop_tx, mut stop_rx) = oneshot::channel();

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(EXECUTION_SLOT_HEARTBEAT_SECONDS));
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match app_state.redis_client.get_multiplexed_async_connection().await {
                        Ok(mut conn) => {
                            let refresh_result: Result<bool, _> = redis::cmd("EXPIRE")
                                .arg(&key)
                                .arg(EXECUTION_SLOT_TTL_SECONDS)
                                .query_async(&mut conn)
                                .await;

                            match refresh_result {
                                Ok(true) => {}
                                Ok(false) => {
                                    tracing::warn!("Execution slot heartbeat lost key {}", key);
                                    break;
                                }
                                Err(error) => {
                                    tracing::warn!("Failed to refresh execution slot heartbeat for {}: {}", key, error);
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!("Failed to get Redis connection for execution slot heartbeat {}: {}", key, error);
                        }
                    }
                }
                _ = &mut stop_rx => break,
            }
        }
    });

    stop_tx
}

fn spawn_context_lock_heartbeat(
    app_state: AppState,
    key: String,
    token: String,
) -> oneshot::Sender<()> {
    let (stop_tx, mut stop_rx) = oneshot::channel();

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(CONTEXT_LOCK_HEARTBEAT_SECONDS));
        ticker.tick().await;
        let refresh_script = Script::new(
            r#"
if redis.call('GET', KEYS[1]) == ARGV[1] then
  return redis.call('EXPIRE', KEYS[1], ARGV[2])
end
return 0
"#,
        );

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    match app_state.redis_client.get_multiplexed_async_connection().await {
                        Ok(mut conn) => {
                            let refresh_result: Result<i64, _> = refresh_script
                                .key(&key)
                                .arg(&token)
                                .arg(CONTEXT_LOCK_TTL_SECONDS)
                                .invoke_async(&mut conn)
                                .await;

                            match refresh_result {
                                Ok(1) => {}
                                Ok(0) => {
                                    tracing::warn!("Context lock heartbeat lost ownership for {}", key);
                                    break;
                                }
                                Ok(other) => {
                                    tracing::warn!("Unexpected context lock heartbeat result for {}: {}", key, other);
                                }
                                Err(error) => {
                                    tracing::warn!("Failed to refresh context lock heartbeat for {}: {}", key, error);
                                }
                            }
                        }
                        Err(error) => {
                            tracing::warn!("Failed to get Redis connection for context lock heartbeat {}: {}", key, error);
                        }
                    }
                }
                _ = &mut stop_rx => break,
            }
        }
    });

    stop_tx
}

async fn acquire_deployment_execution_slot(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentExecutionGuard, AgentExecutionError> {
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

    let started_at = tokio::time::Instant::now();
    let max_wait = Duration::from_secs(MAX_LOCK_WAIT_SECONDS);
    let mut attempt = 0u32;

    loop {
        if started_at.elapsed() >= max_wait {
            return Err(AgentExecutionError::ExecutionBusy {
                resource: "deployment_execution_slot",
                identifier: deployment_id,
                max_wait_seconds: MAX_LOCK_WAIT_SECONDS,
            });
        }

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
            let heartbeat_stop = spawn_deployment_slot_heartbeat(app_state.clone(), key.clone());
            return Ok(DeploymentExecutionGuard {
                app_state: app_state.clone(),
                key,
                heartbeat_stop: Some(heartbeat_stop),
            });
        }

        attempt = attempt.saturating_add(1);
        let backoff_ms = jittered_exponential_backoff_ms(
            attempt,
            DEPLOYMENT_SLOT_BACKOFF_MIN_MS,
            DEPLOYMENT_SLOT_BACKOFF_MAX_MS,
        );
        sleep(Duration::from_millis(backoff_ms)).await;
    }
}

async fn acquire_context_execution_lock(
    app_state: &AppState,
    context_id: i64,
) -> Result<ContextExecutionLockGuard, AgentExecutionError> {
    let key = format!("agent:context_execution_lock:{}", context_id);
    let token = format!(
        "{}:{}",
        context_id,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    );

    let script = Script::new(REDIS_SET_NX_EX_LUA);

    let started_at = tokio::time::Instant::now();
    let max_wait = Duration::from_secs(MAX_LOCK_WAIT_SECONDS);
    let mut attempt = 0u32;

    loop {
        if started_at.elapsed() >= max_wait {
            return Err(AgentExecutionError::ExecutionBusy {
                resource: "context_execution",
                identifier: context_id,
                max_wait_seconds: MAX_LOCK_WAIT_SECONDS,
            });
        }

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
            let heartbeat_stop =
                spawn_context_lock_heartbeat(app_state.clone(), key.clone(), token.clone());
            return Ok(ContextExecutionLockGuard {
                app_state: app_state.clone(),
                key,
                token,
                heartbeat_stop: Some(heartbeat_stop),
            });
        }

        attempt = attempt.saturating_add(1);
        let backoff_ms = jittered_exponential_backoff_ms(
            attempt,
            CONTEXT_LOCK_BACKOFF_MIN_MS,
            CONTEXT_LOCK_BACKOFF_MAX_MS,
        );
        sleep(Duration::from_millis(backoff_ms)).await;
    }
}

fn jittered_exponential_backoff_ms(attempt: u32, base_ms: u64, max_ms: u64) -> u64 {
    let growth_factor = 1u64.checked_shl(attempt.min(8)).unwrap_or(256);
    let exponential = base_ms.saturating_mul(growth_factor).min(max_ms);
    let jitter = time_jitter_ms(BACKOFF_JITTER_MAX_MS);
    exponential.saturating_add(jitter).min(max_ms)
}

fn time_jitter_ms(max_jitter_ms: u64) -> u64 {
    if max_jitter_ms == 0 {
        return 0;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let nanos = now.subsec_nanos() as u64;
    nanos % (max_jitter_ms + 1)
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

    let script = Script::new(REDIS_SET_NX_EX_LUA);

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
