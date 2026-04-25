use agent_engine::{AgentHandler, ExecutionRequest};
use commands::CreateExecutionRunCommand;
use commands::{ApprovalGrantRequest, GrantApprovalGrantsForThreadCommand};
use common::state::AppState;
use dto::json::{AgentExecutionRequest, AgentExecutionType};
use models::{AgentThreadStatus, AiAgentWithFeatures, ThreadEvent};
use queries::{
    GetAgentThreadStateQuery, GetAiAgentByIdWithFeatures, GetConversationByIdQuery,
    GetProjectTaskBoardItemAssignmentByIdQuery, GetProjectTaskBoardItemByIdQuery,
};
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

    pub fn is_unrecoverable(&self) -> bool {
        matches!(self, Self::Unrecoverable(_))
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

#[derive(Debug, Clone)]
enum AgentResolutionStrategy {
    AgentId(i64),
}

impl AgentResolutionStrategy {
    fn display_label(&self) -> String {
        match self {
            Self::AgentId(agent_id) => agent_id.to_string(),
        }
    }
}

#[derive(Debug, Clone)]
enum AgentExecutionKind {
    NewMessage {
        conversation_id: i64,
    },
    ApprovalResponse {
        request_message_id: String,
        approvals: Vec<dto::json::deployment::ToolApprovalSelection>,
    },
    ThreadEvent {
        event_id: i64,
    },
}

#[derive(Debug, Clone)]
struct AgentExecutionEnvelope {
    deployment_id: i64,
    thread_id: i64,
    thread_event_id: Option<i64>,
    execution_token: String,
    agent_resolution: AgentResolutionStrategy,
    execution_kind: AgentExecutionKind,
}

impl TryFrom<AgentExecutionRequest> for AgentExecutionEnvelope {
    type Error = AgentExecutionError;

    fn try_from(request: AgentExecutionRequest) -> Result<Self, Self::Error> {
        let deployment_id = parse_string_id("deployment_id", &request.deployment_id)
            .map_err(AgentExecutionError::unrecoverable)?;
        let thread_id = parse_string_id("thread_id", &request.thread_id)
            .map_err(AgentExecutionError::unrecoverable)?;
        let thread_event_id = request
            .thread_event_id
            .as_ref()
            .map(|value| parse_string_id("thread_event_id", value))
            .transpose()
            .map_err(AgentExecutionError::unrecoverable)?;

        let agent_resolution = match request.agent_id {
            Some(agent_id) => AgentResolutionStrategy::AgentId(
                parse_string_id("agent_id", &agent_id)
                    .map_err(AgentExecutionError::unrecoverable)?,
            ),
            None => {
                return Err(AgentExecutionError::unrecoverable(anyhow::anyhow!(
                    "agent_id must be provided"
                )));
            }
        };

        let execution_kind = match request.execution_type {
            AgentExecutionType::NewMessage { conversation_id } => AgentExecutionKind::NewMessage {
                conversation_id: parse_string_id("conversation_id", &conversation_id)
                    .map_err(AgentExecutionError::unrecoverable)?,
            },
            AgentExecutionType::ApprovalResponse {
                request_message_id,
                approvals,
            } => AgentExecutionKind::ApprovalResponse {
                request_message_id,
                approvals,
            },
            AgentExecutionType::ThreadEvent { event_id } => AgentExecutionKind::ThreadEvent {
                event_id: parse_string_id("event_id", &event_id)
                    .map_err(AgentExecutionError::unrecoverable)?,
            },
        };

        Ok(Self {
            deployment_id,
            thread_id,
            thread_event_id,
            execution_token: String::new(),
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

async fn build_conversation_execution_request(
    thread_id: i64,
    thread_event_id: Option<i64>,
    execution_run_id: i64,
    conversation_id: i64,
    agent: AiAgentWithFeatures,
    execution_token: String,
) -> ExecutionRequest {
    ExecutionRequest {
        agent,
        conversation_id: Some(conversation_id),
        thread_id,
        thread_event_id,
        execution_run_id,
        execution_token,
        approval_response: None,
        thread_event: None,
    }
}

async fn load_thread_event_for_execution(
    app_state: &AppState,
    event_id: i64,
    thread_id: i64,
    deployment_id: i64,
) -> Result<ThreadEvent, AgentExecutionError> {
    let thread_event = queries::GetThreadEventByIdQuery::new(event_id)
        .execute_with_db(
            app_state
                .db_router
                .reader(common::db_router::ReadConsistency::Strong),
        )
        .await
        .map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!(
                "Failed to load thread event {}: {}",
                event_id,
                e
            ))
        })?
        .ok_or_else(|| {
            AgentExecutionError::unrecoverable(anyhow::anyhow!(
                "Thread event {} not found",
                event_id
            ))
        })?;

    if thread_event.thread_id != thread_id {
        return Err(AgentExecutionError::unrecoverable(anyhow::anyhow!(
            "Thread event {} does not belong to thread {}",
            event_id,
            thread_id
        )));
    }

    if thread_event.deployment_id != deployment_id {
        return Err(AgentExecutionError::unrecoverable(anyhow::anyhow!(
            "Thread event {} does not belong to deployment {}",
            event_id,
            deployment_id
        )));
    }

    Ok(thread_event)
}

async fn stale_thread_event_reason(
    app_state: &AppState,
    thread_event: &ThreadEvent,
) -> Result<Option<String>, AgentExecutionError> {
    match thread_event.event_type.as_str() {
        models::thread_event::event_type::TASK_ROUTING => {
            let Some(board_item_id) = thread_event.board_item_id else {
                return Ok(Some(format!(
                    "{} event is missing board_item_id",
                    thread_event.event_type
                )));
            };
            let board_item = GetProjectTaskBoardItemByIdQuery::new(board_item_id)
                .execute_with_db(
                    app_state
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await
                .map_err(|e| {
                    AgentExecutionError::Other(anyhow::anyhow!(
                        "Failed to load board item {} for thread event {}: {}",
                        board_item_id,
                        thread_event.id,
                        e
                    ))
                })?;

            if board_item.is_none() {
                return Ok(Some(format!(
                    "board item {} no longer exists",
                    board_item_id
                )));
            }

            Ok(None)
        }
        models::thread_event::event_type::ASSIGNMENT_EXECUTION => {
            let Some(board_item_id) = thread_event.board_item_id else {
                return Ok(Some(format!(
                    "{} event is missing board_item_id",
                    thread_event.event_type
                )));
            };
            let Some(board_item) = GetProjectTaskBoardItemByIdQuery::new(board_item_id)
                .execute_with_db(
                    app_state
                        .db_router
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await
                .map_err(|e| {
                    AgentExecutionError::Other(anyhow::anyhow!(
                        "Failed to load board item {} for thread event {}: {}",
                        board_item_id,
                        thread_event.id,
                        e
                    ))
                })?
            else {
                return Ok(Some(format!(
                    "board item {} no longer exists",
                    board_item_id
                )));
            };

            let Some(payload) = thread_event.assignment_execution_payload() else {
                return Ok(Some(format!(
                    "{} event payload is invalid",
                    thread_event.event_type
                )));
            };

            let Some(assignment) =
                GetProjectTaskBoardItemAssignmentByIdQuery::new(payload.assignment_id)
                    .execute_with_db(
                        app_state
                            .db_router
                            .reader(common::db_router::ReadConsistency::Strong),
                    )
                    .await
                    .map_err(|e| {
                        AgentExecutionError::Other(anyhow::anyhow!(
                            "Failed to load assignment {} for thread event {}: {}",
                            payload.assignment_id,
                            thread_event.id,
                            e
                        ))
                    })?
            else {
                return Ok(Some(format!(
                    "assignment {} no longer exists",
                    payload.assignment_id
                )));
            };

            if assignment.board_item_id != board_item.id {
                return Ok(Some(format!(
                    "assignment {} belongs to board item {}, not {}",
                    assignment.id, assignment.board_item_id, board_item.id
                )));
            }

            if assignment.thread_id != thread_event.thread_id {
                return Ok(Some(format!(
                    "assignment {} targets thread {}, not event thread {}",
                    assignment.id, assignment.thread_id, thread_event.thread_id
                )));
            }

            if !matches!(
                assignment.status.as_str(),
                models::project_task_board::assignment_status::AVAILABLE
                    | models::project_task_board::assignment_status::CLAIMED
                    | models::project_task_board::assignment_status::IN_PROGRESS
            ) {
                return Ok(Some(format!(
                    "assignment {} is no longer executable (status={})",
                    assignment.id, assignment.status
                )));
            }

            Ok(None)
        }
        _ => Ok(None),
    }
}

async fn reject_stale_thread_event(
    app_state: &AppState,
    thread_event: &ThreadEvent,
    reason: &str,
) -> Result<(), AgentExecutionError> {
    tracing::warn!(
        event_id = thread_event.id,
        thread_id = thread_event.thread_id,
        board_item_id = thread_event.board_item_id,
        event_type = %thread_event.event_type,
        reason = %reason,
        "Rejecting stale thread event before execution"
    );

    commands::UpdateThreadEventStateCommand::new(
        thread_event.id,
        models::thread_event::status::FAILED.to_string(),
    )
    .mark_failed()
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(|e| {
        AgentExecutionError::Other(anyhow::anyhow!(
            "Failed to mark stale thread event {} as failed: {}",
            thread_event.id,
            e
        ))
    })?;

    Ok(())
}

async fn load_live_thread_event_for_execution(
    app_state: &AppState,
    event_id: i64,
    thread_id: i64,
    deployment_id: i64,
) -> Result<Result<ThreadEvent, String>, AgentExecutionError> {
    let thread_event =
        load_thread_event_for_execution(app_state, event_id, thread_id, deployment_id).await?;

    if let Some(reason) = stale_thread_event_reason(app_state, &thread_event).await? {
        reject_stale_thread_event(app_state, &thread_event, &reason).await?;
        return Ok(Err(reason));
    }

    Ok(Ok(thread_event))
}

async fn load_agent_for_execution(
    app_state: &AppState,
    _deployment_id: i64,
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

pub async fn process_agent_execution(
    app_state: &AppState,
    task_id: &str,
    request: AgentExecutionRequest,
) -> Result<String, AgentExecutionError> {
    tracing::info!(
        task_id,
        thread_id = %request.thread_id,
        deployment_id = %request.deployment_id,
        "agent task: received"
    );
    match prepare_agent_execution(app_state, task_id, request).await {
        Ok(PreparedAgentExecutionOutcome::Noop(message)) => {
            tracing::info!(task_id, %message, "agent task: noop");
            Ok(message)
        }
        Ok(PreparedAgentExecutionOutcome::Ready(execution)) => {
            let thread_id = execution.thread_id;
            tracing::info!(task_id, thread_id, "agent task: prepared, executing");
            let started = std::time::Instant::now();
            let result = execution.execute().await;
            tracing::info!(
                task_id,
                thread_id,
                elapsed_ms = started.elapsed().as_millis() as u64,
                ok = result.is_ok(),
                "agent task: execution finished"
            );
            result
        }
        Err(e) => {
            tracing::warn!(task_id, "agent task: prepare failed: {}", e);
            Err(e)
        }
    }
}

pub enum PreparedAgentExecutionOutcome {
    Noop(String),
    Ready(PreparedAgentExecution),
}

pub struct PreparedAgentExecution {
    app_state: AppState,
    agent_identifier: String,
    thread_id: i64,
    execution_request: ExecutionRequest,
    concurrency_guard: DeploymentExecutionGuard,
}

impl PreparedAgentExecution {
    pub async fn execute(self) -> Result<String, AgentExecutionError> {
        let result = AgentHandler::new(self.app_state.clone())
            .execute_agent_streaming(self.execution_request)
            .await;

        drop(self.concurrency_guard);

        result.map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!("Agent execution failed: {}", e))
        })?;

        Ok(format!(
            "Agent '{}' execution completed for thread {}",
            self.agent_identifier, self.thread_id
        ))
    }
}

pub async fn prepare_agent_execution(
    app_state: &AppState,
    task_id: &str,
    request: AgentExecutionRequest,
) -> Result<PreparedAgentExecutionOutcome, AgentExecutionError> {
    let mut execution_envelope = AgentExecutionEnvelope::try_from(request)?;
    let _ = task_id;

    let concurrency_guard =
        acquire_deployment_execution_slot(app_state, execution_envelope.deployment_id).await?;

    let thread = queries::GetAgentThreadByIdQuery::new(
        execution_envelope.thread_id,
        execution_envelope.deployment_id,
    )
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(|e| {
        AgentExecutionError::Other(anyhow::anyhow!(
            "Failed to load thread {}: {}",
            execution_envelope.thread_id,
            e
        ))
    })?
    .ok_or_else(|| {
        AgentExecutionError::unrecoverable(anyhow::anyhow!(
            "Thread {} not found",
            execution_envelope.thread_id
        ))
    })?;

    if thread.thread_purpose == models::agent_thread::purpose::CONVERSATION {
        let token = commands::AdvanceThreadExecutionTokenCommand::new(execution_envelope.thread_id)
            .execute_with_deps(&common::deps::from_app(app_state).nats().id())
            .await
            .map_err(|e| {
                AgentExecutionError::Other(anyhow::anyhow!(
                    "Failed to advance execution token for thread {}: {}",
                    execution_envelope.thread_id,
                    e
                ))
            })?;
        tracing::info!(
            thread_id = execution_envelope.thread_id,
            deployment_id = execution_envelope.deployment_id,
            token_len = token.len(),
            "agent task: advanced execution token"
        );
        execution_envelope.execution_token = token;
    }

    let deployment_id = execution_envelope.deployment_id;
    let thread_id = execution_envelope.thread_id;
    let execution_kind = execution_envelope.execution_kind;
    let thread_event = match &execution_kind {
        AgentExecutionKind::ThreadEvent { event_id } => match load_live_thread_event_for_execution(
            app_state,
            *event_id,
            thread_id,
            deployment_id,
        )
        .await?
        {
            Ok(thread_event) => Some(thread_event),
            Err(reason) => {
                return Ok(PreparedAgentExecutionOutcome::Noop(format!(
                    "Stale thread event {} rejected for thread {}: {}",
                    event_id, thread_id, reason
                )));
            }
        },
        _ => None,
    };
    let agent_identifier = execution_envelope.agent_resolution.display_label();

    let agent = load_agent_for_execution(
        app_state,
        execution_envelope.deployment_id,
        &execution_envelope.agent_resolution,
    )
    .await?;
    let execution_run_id = app_state.sf.next_id().map_err(|e| {
        AgentExecutionError::Other(anyhow::anyhow!(
            "Failed to generate execution run id: {}",
            e
        ))
    })? as i64;

    let create_run_command = CreateExecutionRunCommand::new(
        execution_run_id,
        deployment_id,
        thread_id,
        "running".to_string(),
    )
    .with_agent_id(agent.id);
    create_run_command
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!(
                "Failed to create execution run for thread {}: {}",
                thread_id,
                e
            ))
        })?;

    if let Some(thread_event_id) = execution_envelope.thread_event_id {
        commands::UpdateThreadEventStateCommand::new(
            thread_event_id,
            models::thread_event::status::CLAIMED.to_string(),
        )
        .with_caused_by_run_id(execution_run_id)
        .mark_claimed()
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            AgentExecutionError::Other(anyhow::anyhow!(
                "Failed to claim thread event {} for thread {}: {}",
                thread_event_id,
                thread_id,
                e
            ))
        })?;
    }

    let execution_request = match execution_kind {
        AgentExecutionKind::NewMessage { conversation_id } => {
            build_conversation_execution_request(
                thread_id,
                execution_envelope.thread_event_id,
                execution_run_id,
                conversation_id,
                agent,
                execution_envelope.execution_token.clone(),
            )
            .await
        }
        AgentExecutionKind::ApprovalResponse {
            request_message_id,
            approvals,
        } => {
            persist_tool_approval_response_grants(
                app_state,
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
                thread_event_id: execution_envelope.thread_event_id,
                execution_run_id,
                execution_token: execution_envelope.execution_token.clone(),
                approval_response: Some(approvals),
                thread_event: None,
            }
        }
        AgentExecutionKind::ThreadEvent { event_id } => {
            let thread_event = thread_event.ok_or_else(|| {
                AgentExecutionError::Other(anyhow::anyhow!(
                    "Preloaded thread event {} missing for thread {}",
                    event_id,
                    thread_id
                ))
            })?;

            ExecutionRequest {
                agent,
                conversation_id: None,
                thread_id,
                thread_event_id: execution_envelope.thread_event_id.or(Some(event_id)),
                execution_run_id,
                execution_token: execution_envelope.execution_token.clone(),
                approval_response: None,
                thread_event: Some(thread_event),
            }
        }
    };

    Ok(PreparedAgentExecutionOutcome::Ready(
        PreparedAgentExecution {
            app_state: app_state.clone(),
            agent_identifier,
            thread_id,
            execution_request,
            concurrency_guard,
        },
    ))
}

struct DeploymentExecutionGuard {
    app_state: AppState,
    key: String,
    heartbeat_stop: Option<oneshot::Sender<()>>,
}

impl Drop for DeploymentExecutionGuard {
    fn drop(&mut self) {
        tracing::info!(key = %self.key, "agent task: releasing execution slot");
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
