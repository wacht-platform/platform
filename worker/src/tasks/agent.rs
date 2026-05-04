use agent_engine::{AgentHandler, ExecutionRequest};
use commands::{ApprovalGrantRequest, GrantApprovalGrantsForThreadCommand};
use common::state::AppState;
use models::{AgentThreadStatus, ThreadEvent};
use queries::{GetAgentThreadStateQuery, GetAiAgentByIdWithFeatures, GetConversationByIdQuery};
use redis::Script;
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio::time::{Duration, interval, sleep};

const MAX_DEPLOYMENT_CONCURRENT_EXECUTIONS: i64 = 2000;
const EXECUTION_SLOT_TTL_SECONDS: i64 = 600;
const EXECUTION_SLOT_HEARTBEAT_SECONDS: u64 = 120;
const MAX_LOCK_WAIT_SECONDS: u64 = 300; // 5 minutes
const DEPLOYMENT_SLOT_BACKOFF_MIN_MS: u64 = 250;
const DEPLOYMENT_SLOT_BACKOFF_MAX_MS: u64 = 5_000;
const BACKOFF_JITTER_MAX_MS: u64 = 250;

#[derive(Debug)]
pub enum AgentExecutionError {
    ExecutionBusy {
        resource: &'static str,
        identifier: i64,
        max_wait_seconds: u64,
    },
    /// Reserved for deterministic failures that will still fail on retry:
    /// malformed payloads, references to rows that no longer exist, validation
    /// breaches. When the consumer sees this it marks the event terminally
    /// failed without letting the recovery cron burn the retry budget.
    Unrecoverable(anyhow::Error),
    /// Default fallback. Transient / unclassified errors. The recovery cron
    /// will re-pend the event until `max_retries` is exhausted, then mark it
    /// failed.
    Other(anyhow::Error),
}

impl AgentExecutionError {
    pub fn unrecoverable<E: Into<anyhow::Error>>(error: E) -> Self {
        Self::Unrecoverable(error.into())
    }
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
            Self::Unrecoverable(err) => write!(f, "Unrecoverable: {err}"),
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


async fn persist_tool_approval_response_grants(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    request_message_id: &str,
    approvals: &[dto::json::deployment::ToolApprovalSelection],
) -> Result<(), AgentExecutionError> {
    let thread_state = GetAgentThreadStateQuery::new(thread_id, deployment_id)
        .execute_with_db(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await
        .map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!(
                "Failed to load thread {}: {}",
                thread_id,
                e
            ))
        })?;
    let has_pending_approval_request = thread_state
        .execution_state
        .as_ref()
        .and_then(|state| state.pending_approval_request.as_ref())
        .is_some();

    if thread_state.status != AgentThreadStatus::WaitingForInput && !has_pending_approval_request {
        return Err(AgentExecutionError::Other(anyhow::anyhow!(
            "Approval responses are only accepted while the thread is waiting for input"
        )));
    }

    let parsed_request_message_id = request_message_id
        .parse::<i64>()
        .map_err(|_| AgentExecutionError::Other(anyhow::anyhow!("Invalid request_message_id")))?;

    let request_conversation = GetConversationByIdQuery::new(parsed_request_message_id)
        .execute_with_db(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await
        .map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!(
                "Failed to load approval request conversation {}: {}",
                parsed_request_message_id,
                e
            ))
        })?;

    if request_conversation.thread_id != Some(thread_id) {
        return Err(AgentExecutionError::Other(anyhow::anyhow!(
            "Approval request does not belong to this thread"
        )));
    }

    let requested_tools = match request_conversation.content {
        models::ConversationContent::ApprovalRequest { tools, .. } => tools
            .into_iter()
            .map(|tool| (tool.tool_name, tool.tool_id))
            .collect::<HashMap<_, _>>(),
        _ => {
            return Err(AgentExecutionError::Other(anyhow::anyhow!(
                "request_message_id must reference an approval_request message"
            )));
        }
    };

    let mut seen = HashSet::new();
    let mut grants = Vec::new();

    for approval in approvals {
        if approval.tool_name.trim().is_empty() {
            return Err(AgentExecutionError::Other(anyhow::anyhow!(
                "Approval response tool names must be non-empty"
            )));
        }
        if !seen.insert(approval.tool_name.clone()) {
            return Err(AgentExecutionError::Other(anyhow::anyhow!(
                "Approval response contains duplicate tool '{}'",
                approval.tool_name
            )));
        }

        let tool_id = requested_tools
            .get(&approval.tool_name)
            .copied()
            .ok_or_else(|| {
                AgentExecutionError::Other(anyhow::anyhow!(
                    "Tool '{}' was not part of the pending approval request",
                    approval.tool_name
                ))
            })?;

        grants.push(ApprovalGrantRequest {
            tool_id,
            mode: approval.mode,
        });
    }

    if grants.is_empty() {
        return Ok(());
    }

    GrantApprovalGrantsForThreadCommand::new(deployment_id, thread_id, grants)
        .execute_with_deps(&common::deps::from_app(app_state).db().id())
        .await
        .map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!(
                "Failed to persist tool approvals for thread {}: {}",
                thread_id,
                e
            ))
        })?;

    Ok(())
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
        if should_log_wait_attempt(attempt) {
            let current_active_executions: Option<String> = redis::cmd("GET")
                .arg(&key)
                .query_async(&mut conn)
                .await
                .ok();
            tracing::warn!(
                deployment_id,
                redis_key = %key,
                attempt,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
                backoff_ms,
                ?current_active_executions,
                max_active = MAX_DEPLOYMENT_CONCURRENT_EXECUTIONS,
                "Blocked waiting for deployment execution slot"
            );
        }
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

fn should_log_wait_attempt(attempt: u32) -> bool {
    attempt <= 3 || attempt % 10 == 0
}

const EVENT_LOG_HEARTBEAT_SECONDS: u64 = 120;

pub async fn process_event_log_work(
    app_state: &AppState,
    task_id: &str,
    payload: serde_json::Value,
) -> Result<String, AgentExecutionError> {
    let event_log_id = parse_payload_i64(&payload, "event_log_id")?;
    let deployment_id = parse_payload_i64(&payload, "deployment_id")?;
    let thread_id = parse_payload_i64(&payload, "thread_id")?;
    let kind = payload
        .get("kind")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            AgentExecutionError::unrecoverable(anyhow::anyhow!("missing event_log kind"))
        })?
        .to_string();

    let worker_id = format!("worker-{}-{}", std::process::id(), task_id);
    let claimed = commands::event_log::claim_work_lease(
        app_state.db_router.writer(),
        event_log_id,
        &worker_id,
        commands::event_log::DEFAULT_LEASE_SECONDS,
    )
    .await
    .map_err(|e| AgentExecutionError::Other(anyhow::anyhow!("claim work_lease: {e}")))?;

    if !claimed {
        return Ok(format!("event_log {event_log_id}: already leased"));
    }

    let (heartbeat_handle, heartbeat_stop) = spawn_event_log_heartbeat(
        app_state.clone(),
        event_log_id,
        worker_id.clone(),
    );

    let outcome = run_event_log_work(
        app_state.clone(),
        event_log_id,
        deployment_id,
        thread_id,
        &kind,
        payload,
    )
    .await;

    let _ = heartbeat_stop.send(());
    let _ = heartbeat_handle.await;

    match &outcome {
        Ok(_) => {
            let _ = commands::event_log::release_work_lease(
                app_state.db_router.writer(),
                event_log_id,
            )
            .await;
        }
        Err(e) => {
            tracing::warn!(event_log_id, error = %e, "event_log work failed; releasing lease for retry");
            let _ = commands::event_log::release_work_lease(
                app_state.db_router.writer(),
                event_log_id,
            )
            .await;
        }
    }

    outcome
}

async fn run_event_log_work(
    app_state: AppState,
    event_log_id: i64,
    deployment_id: i64,
    thread_id: i64,
    kind: &str,
    payload: serde_json::Value,
) -> Result<String, AgentExecutionError> {
    let concurrency_guard = acquire_deployment_execution_slot(&app_state, deployment_id).await?;

    let thread = queries::GetAgentThreadByIdQuery::new(thread_id, deployment_id)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| AgentExecutionError::Other(anyhow::anyhow!("load thread: {e}")))?
        .ok_or_else(|| {
            AgentExecutionError::unrecoverable(anyhow::anyhow!(
                "thread {thread_id} not found in deployment {deployment_id}"
            ))
        })?;

    let agent_id_override = payload
        .get("agent_id")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok());
    let agent_id = match agent_id_override {
        Some(id) => id,
        None => resolve_agent_id_for_thread(&app_state, thread_id).await?,
    };
    let agent = GetAiAgentByIdWithFeatures::new(agent_id)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| AgentExecutionError::Other(anyhow::anyhow!("load agent: {e}")))?;

    let execution_run_id = app_state
        .sf
        .next_id()
        .map_err(|e| AgentExecutionError::Other(anyhow::anyhow!("snowflake: {e}")))?
        as i64;

    commands::CreateExecutionRunCommand::new(
        execution_run_id,
        deployment_id,
        thread_id,
        "running".to_string(),
    )
    .with_agent_id(agent.id)
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(|e| AgentExecutionError::Other(anyhow::anyhow!("create execution_run: {e}")))?;

    let (execution_token, watch_key) =
        if thread.thread_purpose == models::agent_thread::purpose::CONVERSATION {
            let token = commands::AdvanceThreadExecutionTokenCommand::new(thread_id)
                .execute_with_deps(&common::deps::from_app(&app_state).nats().id())
                .await
                .map_err(|e| {
                    AgentExecutionError::Other(anyhow::anyhow!("advance execution token: {e}"))
                })?;
            (token, thread_id.to_string())
        } else {
            let key = format!("event_log_{event_log_id}");
            let token = event_log_id.to_string();
            commands::write_execution_watch_key(&app_state.nats_jetstream, &key, &token)
                .await
                .map_err(|e| {
                    AgentExecutionError::Other(anyhow::anyhow!("write execution watch key: {e}"))
                })?;
            (token, key)
        };

    let request = match kind {
        "task_routing" | "assignment_execution" => {
            let board_item_id = payload
                .get("board_item_id")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i64>().ok());
            let synthetic_payload = if kind == "task_routing" {
                let bid = board_item_id.ok_or_else(|| {
                    AgentExecutionError::unrecoverable(anyhow::anyhow!(
                        "task_routing missing board_item_id"
                    ))
                })?;
                let mut p = payload.clone();
                if let Some(obj) = p.as_object_mut() {
                    obj.insert(
                        "board_item_id".to_string(),
                        serde_json::Value::String(bid.to_string()),
                    );
                }
                p
            } else {
                let aid = parse_payload_i64(&payload, "assignment_id")?;
                serde_json::json!({ "assignment_id": aid.to_string() })
            };

            let synthetic_event = ThreadEvent {
                id: event_log_id,
                deployment_id,
                thread_id,
                board_item_id,
                event_type: kind.to_string(),
                payload: synthetic_payload,
                caused_by_thread_id: None,
            };

            ExecutionRequest {
                agent,
                conversation_id: None,
                thread_id,
                event_log_id: Some(event_log_id),
                execution_run_id,
                execution_token: execution_token.clone(),
                watch_key: watch_key.clone(),
                approval_response: None,
                thread_event: Some(synthetic_event),
            }
        }
        "user_message_received" => {
            let conversation_id = parse_payload_i64(&payload, "conversation_id")?;
            ExecutionRequest {
                agent,
                conversation_id: Some(conversation_id),
                thread_id,
                event_log_id: Some(event_log_id),
                execution_run_id,
                execution_token: execution_token.clone(),
                watch_key: watch_key.clone(),
                approval_response: None,
                thread_event: None,
            }
        }
        "approval_response_received" => {
            let exec_request: dto::json::AgentExecutionRequest = payload
                .get("execution_payload")
                .cloned()
                .ok_or_else(|| {
                    AgentExecutionError::unrecoverable(anyhow::anyhow!(
                        "approval_response_received missing execution_payload"
                    ))
                })
                .and_then(|v| {
                    serde_json::from_value(v).map_err(|e| {
                        AgentExecutionError::unrecoverable(anyhow::anyhow!(
                            "invalid execution_payload: {e}"
                        ))
                    })
                })?;
            let (request_message_id, approvals) = match exec_request.execution_type {
                dto::json::AgentExecutionType::ApprovalResponse {
                    request_message_id,
                    approvals,
                } => (request_message_id, approvals),
                other => {
                    return Err(AgentExecutionError::unrecoverable(anyhow::anyhow!(
                        "approval_response_received expected ApprovalResponse, got {other:?}"
                    )));
                }
            };
            persist_tool_approval_response_grants(
                &app_state,
                deployment_id,
                thread_id,
                &request_message_id,
                &approvals,
            )
            .await?;
            ExecutionRequest {
                agent,
                conversation_id: None,
                thread_id,
                event_log_id: Some(event_log_id),
                execution_run_id,
                execution_token: execution_token.clone(),
                watch_key: watch_key.clone(),
                approval_response: Some(approvals),
                thread_event: None,
            }
        }
        other => {
            return Err(AgentExecutionError::unrecoverable(anyhow::anyhow!(
                "unknown event_log kind: {other}"
            )));
        }
    };

    let result = AgentHandler::new(app_state.clone())
        .execute_agent_streaming(request)
        .await;

    drop(concurrency_guard);

    match result {
        Ok(_) => Ok(format!("event_log {event_log_id} ({kind}) completed")),
        Err(e) => Err(AgentExecutionError::Other(anyhow::anyhow!(
            "agent execution failed for event_log {event_log_id}: {e}"
        ))),
    }
}


async fn resolve_agent_id_for_thread(
    app_state: &AppState,
    thread_id: i64,
) -> Result<i64, AgentExecutionError> {
    let assignment = queries::GetThreadAgentAssignmentQuery::new(thread_id)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!("load thread_agent_assignment: {e}"))
        })?;

    assignment.map(|a| a.agent_id).ok_or_else(|| {
        AgentExecutionError::unrecoverable(anyhow::anyhow!(
            "no agent assigned to thread {thread_id}"
        ))
    })
}

fn parse_payload_i64(payload: &serde_json::Value, field: &str) -> Result<i64, AgentExecutionError> {
    payload
        .get(field)
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .ok_or_else(|| {
            AgentExecutionError::unrecoverable(anyhow::anyhow!(
                "missing or invalid event_log payload field: {field}"
            ))
        })
}

fn spawn_event_log_heartbeat(
    app_state: AppState,
    event_log_id: i64,
    worker_id: String,
) -> (
    tokio::task::JoinHandle<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
    let handle = tokio::spawn(async move {
        let mut tick = interval(Duration::from_secs(EVENT_LOG_HEARTBEAT_SECONDS));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = &mut rx => return,
                _ = tick.tick() => {
                    let result = commands::event_log::heartbeat_work_lease(
                        app_state.db_router.writer(),
                        event_log_id,
                        &worker_id,
                        commands::event_log::DEFAULT_LEASE_SECONDS,
                    )
                    .await;
                    match result {
                        Ok(false) => {
                            tracing::warn!(event_log_id, "lease lost during heartbeat");
                            return;
                        }
                        Err(e) => {
                            tracing::warn!(event_log_id, error = %e, "heartbeat failed");
                        }
                        _ => {}
                    }
                }
            }
        }
    });
    (handle, tx)
}
