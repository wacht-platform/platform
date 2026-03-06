use commands::{CreateChildContextCommand, UpdateExecutionContextQuery};
use common::error::AppError;
use models::AgentExecutionContext;
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;

use crate::execution_context::ExecutionContext;

use super::guards;
use super::response;
use super::TriggerContextRequest;

#[derive(Serialize)]
struct RelayResultPayload {
    message: String,
    target_context_id: i64,
    agent_name: String,
    execution_triggered: bool,
    created_temporary_context: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_context_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relayed_message: Option<String>,
}

#[derive(Serialize)]
struct RelayResponseData {
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    result: RelayResultPayload,
}

#[derive(Serialize)]
struct RelayDedupMeta {
    deduplicated: bool,
    result: RelayResultPayload,
}

pub async fn relay_to_context(
    execution_context: Arc<ExecutionContext>,
    tool_name: &str,
    request: TriggerContextRequest,
) -> Result<Value, AppError> {
    let app_state = &execution_context.app_state;
    let current_context_id = execution_context.context_id;
    let current_agent = &execution_context.agent;
    let requested_target_context_id = request.target_context_id.map(|v| v.0);
    let requested_agent_name = request.normalized_agent_name();

    let dedupe_key = relay_dedupe_key(current_agent.deployment_id, current_context_id, &request);
    if guards::acquire_dedupe_token(app_state, &dedupe_key, 20).await? {
        return response::success(
            tool_name,
            RelayDedupMeta {
                deduplicated: true,
                result: RelayResultPayload {
                    message: "Duplicate relay request ignored".to_string(),
                    target_context_id: requested_target_context_id.unwrap_or(0),
                    agent_name: requested_agent_name,
                    execution_triggered: false,
                    created_temporary_context: false,
                    target_context_title: None,
                    relayed_message: None,
                },
            },
        );
    }

    let relay_result = execute_relay(
        execution_context.clone(),
        tool_name,
        current_context_id,
        current_agent,
        request,
    )
    .await;

    if relay_result.is_err() {
        guards::clear_token(app_state, &dedupe_key).await;
    }

    relay_result
}

async fn execute_relay(
    execution_context: Arc<ExecutionContext>,
    tool_name: &str,
    current_context_id: i64,
    current_agent: &models::AiAgentWithFeatures,
    request: TriggerContextRequest,
) -> Result<Value, AppError> {
    let app_state = &execution_context.app_state;
    let instruction_text = request.instruction_text().ok_or_else(|| {
        AppError::BadRequest(
            "Missing required cross-context instructions. Provide `instructions`.".to_string(),
        )
    })?;
    let resolved_agent_name =
        resolve_execution_agent_name(execution_context.clone(), current_agent, &request).await?;
    let is_fork_mode = request.is_fork_mode();

    if is_fork_mode {
        if request.target_context_id.is_some() {
            return Err(AppError::BadRequest(
                "fork mode does not accept target_context_id; it always runs in the current context"
                    .to_string(),
            ));
        }
        if !request.execute {
            return Err(AppError::BadRequest(
                "fork mode requires execute=true".to_string(),
            ));
        }
        if resolved_agent_name.eq_ignore_ascii_case(&current_agent.name) {
            return Err(AppError::BadRequest(
                "fork mode requires switching to another agent (agent_name cannot be self/current agent)"
                    .to_string(),
            ));
        }
    }

    let (target_context, created_temporary_context) = if is_fork_mode {
        (execution_context.get_context().await?, false)
    } else if let Some(target_context_id) = request.target_context_id.map(|v| v.0) {
        let target_context: AgentExecutionContext = execution_context
            .get_context_by_id(target_context_id)
            .await
            .map_err(|_| {
                AppError::BadRequest(format!(
                    "Target context {} not found or not accessible",
                    target_context_id
                ))
            })?;
        (target_context, false)
    } else {
        let title = format!(
            "Spawned: {} via {}",
            resolved_agent_name, current_agent.name
        );
        let create_child_command =
            CreateChildContextCommand::new(current_agent.deployment_id, current_context_id, title)
                .with_initial_task(instruction_text.to_string())
                .with_task_type("spawn_context_execution".to_string());
        let child_context = create_child_command
            .execute_with(app_state.db_router.writer(), app_state.sf.next_id()? as i64)
            .await?;

        // Child context inherits parent conversation up to this point (without copying rows).
        let history_query = queries::GetLLMConversationHistoryQuery::new(current_context_id);
        let parent_history = history_query
            .execute_with(
                app_state
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await
            .unwrap_or_default();
        let inherit_until = parent_history.last().map(|c| c.id).unwrap_or(0);
        let update_context_command =
            UpdateExecutionContextQuery::new(child_context.id, current_agent.deployment_id)
                .with_external_resource_metadata(serde_json::json!({
                    "inherit_parent_context_id": current_context_id,
                    "inherit_parent_until_conversation_id": inherit_until,
                }));
        update_context_command.execute_with_deps(app_state).await?;

        (child_context, true)
    };
    let target_context_id = target_context.id;

    let conversation_id = app_state
        .sf
        .next_id()
        .map_err(|error| AppError::Internal(format!("Failed to generate ID: {}", error)))?
        as i64;

    let relayed_message = if is_fork_mode {
        format!(
            "[Fork handoff from agent '{}' in context #{}] {}",
            current_agent.name, current_context_id, instruction_text
        )
    } else {
        format!(
            "[Cross-context message from context #{}] {}",
            current_context_id, instruction_text
        )
    };

    let content = models::ConversationContent::UserMessage {
        message: relayed_message,
        sender_name: Some(format!("Cross-context relay from #{}", current_context_id)),
        files: None,
    };

    let create_conversation_command = commands::CreateConversationCommand::new(
        conversation_id,
        target_context_id,
        content,
        models::ConversationMessageType::UserMessage,
    );
    create_conversation_command
        .execute_with(app_state.db_router.writer())
        .await?;

    if request.execute {
        let publish_command = commands::PublishAgentExecutionCommand::new_message(
            current_agent.deployment_id,
            target_context_id,
            None,
            Some(resolved_agent_name.clone()),
            conversation_id,
        );
        publish_command
            .execute_with(&app_state.nats_jetstream, app_state.sf.next_id()? as i64)
            .await?;
    }

    response::success(
        tool_name,
        RelayResponseData {
            status: if is_fork_mode {
                Some("pending".to_string())
            } else {
                None
            },
            result: RelayResultPayload {
                message: if request.execute {
                    if is_fork_mode {
                        "Fork handoff triggered in current context; current agent should pause and supervisor control transfers to selected agent".to_string()
                    } else if created_temporary_context {
                        "Temporary context created, message relayed, and execution triggered"
                            .to_string()
                    } else {
                        "Message relayed and execution triggered".to_string()
                    }
                } else {
                    "Message relayed to target context".to_string()
                },
                target_context_id,
                agent_name: resolved_agent_name,
                execution_triggered: request.execute,
                created_temporary_context,
                target_context_title: Some(target_context.title),
                relayed_message: Some(instruction_text.to_string()),
            },
        },
    )
}

fn relay_dedupe_key(
    deployment_id: i64,
    current_context_id: i64,
    request: &TriggerContextRequest,
) -> String {
    let instruction_text = request.instruction_text();
    let fingerprint = guards::stable_fingerprint(&(
        &request.target_context_id.map(|v| v.0),
        &request.agent_name,
        &request.is_fork_mode(),
        &request.execute,
        &instruction_text,
    ));

    format!(
        "agent_swarm:relay_dedupe:{}:{}:{}",
        deployment_id, current_context_id, fingerprint
    )
}

async fn resolve_execution_agent_name(
    execution_context: Arc<ExecutionContext>,
    current_agent: &models::AiAgentWithFeatures,
    request: &TriggerContextRequest,
) -> Result<String, AppError> {
    let requested = request.normalized_agent_name();
    if requested.is_empty() {
        return Err(AppError::BadRequest(
            "Missing required `agent_name` ('self' or configured sub-agent name)".to_string(),
        ));
    }

    if requested.eq_ignore_ascii_case("self") {
        return Ok(current_agent.name.clone());
    }

    let sub_agent_ids = current_agent.sub_agents.clone().unwrap_or_default();
    if sub_agent_ids.is_empty() {
        return Err(AppError::BadRequest(
            "No sub-agents are configured for this agent".to_string(),
        ));
    }

    let candidates =
        queries::GetAiAgentsByIdsQuery::new(current_agent.deployment_id, sub_agent_ids)
            .execute_with(
                execution_context
                    .app_state
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?;

    candidates
        .into_iter()
        .find(|agent| agent.name.eq_ignore_ascii_case(&requested))
        .map(|agent| agent.name)
        .ok_or_else(|| {
            AppError::BadRequest(format!(
                "Unknown sub-agent '{}'. Use 'self' or a configured sub-agent name.",
                requested
            ))
        })
}
