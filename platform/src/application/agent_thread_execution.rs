use commands::agent_execution::UploadFilesToS3Command;
use commands::{
    CreateConversationCommand, CreateProjectTaskBoardItemEventCommand, DispatchThreadEventCommand,
    DispatchThreadEventResult, EnqueueThreadEventCommand,
    EnsurePulseUsageAllowedForDeploymentCommand, ThreadEventWakeDisposition,
    UpdateAgentThreadStateCommand,
};
use common::ReadConsistency;
use common::error::AppError;
use dto::json::deployment::{ExecuteAgentRequest, ExecuteAgentResponse};
use models::plan_features::PlanFeature;
use models::{
    AgentThreadStatus, ConversationContent, RequestedToolApprovalState, ToolApprovalDecision,
    ToolApprovalRequestState,
};
use queries::{
    GetAgentThreadStateQuery, GetConversationByIdQuery, GetDeploymentAiSettingsQuery,
    GetRecentConversationsQuery, plan_access::CheckDeploymentFeatureAccessQuery,
};
use std::collections::{HashMap, HashSet};
use tracing::{error, info};

use crate::application::AppState;
use common::deps;

const EXECUTION_VARIANT_VALIDATION_ERROR: &str =
    "Exactly one execution_type variant must be provided";

fn queued_execution_response(conversation_id: Option<i64>) -> ExecuteAgentResponse {
    ExecuteAgentResponse {
        status: "queued".to_string(),
        conversation_id: conversation_id.map(|id| id.to_string()),
    }
}

fn next_conversation_id(app_state: &AppState) -> Result<i64, AppError> {
    Ok(app_state
        .sf
        .next_id()
        .map_err(|e| AppError::Internal(format!("Failed to generate conversation ID: {}", e)))?
        as i64)
}

fn parse_pending_approval_request(
    conversation: models::ConversationRecord,
    thread_id: i64,
) -> Result<ToolApprovalRequestState, AppError> {
    if conversation.thread_id != thread_id {
        return Err(AppError::BadRequest(
            "Approval request does not belong to this thread".to_string(),
        ));
    }

    if let ConversationContent::ApprovalRequest { description, tools } = conversation.content {
        return Ok(ToolApprovalRequestState {
            request_message_id: Some(conversation.id.to_string()),
            description,
            tools: tools
                .into_iter()
                .map(|tool| RequestedToolApprovalState {
                    tool_id: tool.tool_id,
                    tool_name: tool.tool_name,
                    tool_description: tool.tool_description,
                })
                .collect(),
        });
    }

    Err(AppError::BadRequest(
        "request_message_id must reference an approval_request message".to_string(),
    ))
}

async fn enqueue_thread_event(
    app_state: &AppState,
    thread_state: &models::AgentThreadState,
    thread_id: i64,
    event_type: &str,
    priority: i32,
    payload: serde_json::Value,
    conversation_id: Option<i64>,
    agent_id: Option<i64>,
) -> Result<DispatchThreadEventResult, AppError> {
    let command = DispatchThreadEventCommand::new(
        EnqueueThreadEventCommand::new(
            app_state.sf.next_id()? as i64,
            thread_state.deployment_id,
            thread_id,
            event_type.to_string(),
        )
        .with_priority(priority)
        .with_payload(payload),
    );
    let command = if let Some(agent_id) = agent_id {
        command.with_agent_id(agent_id)
    } else {
        command
    };

    let event = command
        .execute_with_deps(&deps::from_app(app_state).db().nats().id())
        .await?;

    if let Some(board_item_id) = event.event.board_item_id {
        let summary = match event_type {
            "user_message_received" => "User message received",
            "user_input_received" => "User input received",
            "approval_response_received" => "Approval response received",
            models::thread_event::event_type::CONTROL_STOP => "Thread control stop requested",
            _ => "Thread event received",
        };

        let mut details = serde_json::json!({
            "event_type": event_type,
            "thread_id": thread_id,
        });
        if let Some(conversation_id) = conversation_id {
            details["conversation_id"] = serde_json::json!(conversation_id);
        }

        CreateProjectTaskBoardItemEventCommand {
            id: app_state.sf.next_id()? as i64,
            board_item_id,
            thread_id: Some(thread_id),
            execution_run_id: None,
            event_type: event_type.to_string(),
            summary: summary.to_string(),
            body_markdown: None,
            details,
        }
        .execute_with_db(app_state.db_router.writer())
        .await?;
    }

    Ok(event)
}

async fn ensure_pulse_usage_if_needed(
    app_state: &AppState,
    deployment_id: i64,
    has_custom_gemini_key: bool,
) -> Result<(), AppError> {
    if !has_custom_gemini_key {
        EnsurePulseUsageAllowedForDeploymentCommand::new(deployment_id)
            .execute_with_db(app_state.db_router.writer())
            .await?;
    }
    Ok(())
}

async fn enqueue_thread_control_event_if_needed(
    app_state: &AppState,
    thread_state: &models::AgentThreadState,
    should_signal: bool,
    event_type: &str,
) -> Result<(), AppError> {
    if should_signal {
        let command = DispatchThreadEventCommand::new(
            EnqueueThreadEventCommand::new(
                app_state.sf.next_id()? as i64,
                thread_state.deployment_id,
                thread_state.id,
                event_type.to_string(),
            )
            .with_priority(0)
            .with_payload(serde_json::json!({})),
        );

        command
            .execute_with_deps(&deps::from_app(app_state).db().nats().id())
            .await?;
    }
    Ok(())
}

pub async fn execute_agent_async(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    request: ExecuteAgentRequest,
) -> Result<ExecuteAgentResponse, AppError> {
    use dto::json::deployment::ExecuteAgentRequestType;
    use models::{ConversationContent, ConversationMessageType};

    let thread_state = GetAgentThreadStateQuery::new(thread_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;
    let has_active_execution = thread_state.status.is_active();

    let ExecuteAgentRequestType {
        new_message,
        approval_response,
        cancel,
    } = request.execution_type;

    let execution_variants = [
        new_message.is_some(),
        approval_response.is_some(),
        cancel.is_some(),
    ];

    if execution_variants.iter().filter(|&&v| v).count() != 1 {
        return Err(AppError::BadRequest(
            EXECUTION_VARIANT_VALIDATION_ERROR.to_string(),
        ));
    }

    if cancel.is_none() {
        let has_ai_access =
            CheckDeploymentFeatureAccessQuery::new(deployment_id, PlanFeature::AiAgents)
                .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
                .await
                .map_err(|e| {
                    AppError::Internal(format!("Failed to check AI feature access: {}", e))
                })?;

        if !has_ai_access {
            return Err(AppError::Forbidden(
                "AI agent usage requires Growth plan".to_string(),
            ));
        }
    }

    let has_custom_gemini_key = if cancel.is_none() {
        GetDeploymentAiSettingsQuery::new(deployment_id)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
            .await?
            .and_then(|s| s.gemini_api_key)
            .is_some()
    } else {
        false
    };

    let agent_id = request
        .agent_id
        .as_deref()
        .map(|value| {
            value
                .parse::<i64>()
                .map_err(|_| AppError::BadRequest("Invalid agent_id".to_string()))
        })
        .transpose()?;

    match (new_message, approval_response, cancel) {
        (Some(new_message), None, None) => {
            ensure_pulse_usage_if_needed(app_state, deployment_id, has_custom_gemini_key).await?;
            match thread_state.status {
                AgentThreadStatus::Running => {
                    enqueue_thread_control_event_if_needed(
                        app_state,
                        &thread_state,
                        true,
                        models::thread_event::event_type::CONTROL_INTERRUPT,
                    )
                    .await?;
                }
                AgentThreadStatus::WaitingForInput => {
                    let mut update = UpdateAgentThreadStateCommand::new(thread_id, deployment_id)
                        .with_status(AgentThreadStatus::Interrupted);
                    if let Some(mut execution_state) = thread_state.execution_state.clone() {
                        execution_state.pending_approval_request = None;
                        update = update.with_execution_state(execution_state);
                    }
                    update
                        .execute_with_deps(&deps::from_app(app_state).db().nats().id())
                        .await?;
                }
                _ => {}
            }

            if new_message.message.trim().is_empty()
                && new_message
                    .files
                    .as_ref()
                    .map(|files: &Vec<dto::json::agent_executor::FileData>| files.is_empty())
                    .unwrap_or(true)
            {
                return Err(AppError::BadRequest(
                    "Message or files required".to_string(),
                ));
            }

            let upload_files_command =
                UploadFilesToS3Command::new(deployment_id, thread_id, new_message.files);
            let upload_deps = deps::from_app(app_state).db().enc().id();
            let model_files = match upload_files_command.execute_with_deps(&upload_deps).await {
                Ok(files) => files,
                Err(e) => {
                    error!("Failed to upload files: {}", e);
                    None
                }
            };

            let conversation_id = next_conversation_id(app_state)?;
            let message = if new_message.message.trim().is_empty() {
                let file_names = model_files
                    .as_ref()
                    .map(|files| {
                        files
                            .iter()
                            .map(|f| f.filename.clone())
                            .collect::<Vec<String>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                format!("I've uploaded the following files: {}", file_names)
            } else {
                new_message.message
            };

            CreateConversationCommand::new(
                conversation_id,
                thread_id,
                ConversationContent::UserMessage {
                    message,
                    sender_name: None,
                    files: model_files,
                },
                ConversationMessageType::UserMessage,
            )
            .execute_with_db(app_state.db_router.writer())
            .await?;

            let thread_event = enqueue_thread_event(
                app_state,
                &thread_state,
                thread_id,
                "user_message_received",
                70,
                serde_json::json!({
                    "message_type": "user_message",
                    "conversation_id": conversation_id,
                }),
                Some(conversation_id),
                agent_id,
            )
            .await?;

            if thread_event.wake_disposition == ThreadEventWakeDisposition::Published {
                info!(
                    "Published new_message execution for thread {} (conversation_id: {})",
                    thread_id, conversation_id
                );
            } else {
                info!(
                    "Queued user_message event {} for busy thread {} (conversation_id: {})",
                    thread_event.event.id, thread_id, conversation_id
                );
            }

            Ok(queued_execution_response(Some(conversation_id)))
        }
        (None, Some(approval_response), None) => {
            ensure_pulse_usage_if_needed(app_state, deployment_id, has_custom_gemini_key).await?;
            let has_pending_approval_request = thread_state
                .execution_state
                .as_ref()
                .and_then(|state| state.pending_approval_request.as_ref())
                .is_some();

            if thread_state.status != AgentThreadStatus::WaitingForInput
                && !has_pending_approval_request
            {
                return Err(AppError::BadRequest(
                    "Approval responses are only accepted while the thread is waiting for input"
                        .to_string(),
                ));
            }

            let request_message_id = approval_response
                .request_message_id
                .parse::<i64>()
                .map_err(|_| AppError::BadRequest("Invalid request_message_id".to_string()))?;

            let request_conversation = GetConversationByIdQuery::new(request_message_id)
                .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
                .await?;

            let pending_request = parse_pending_approval_request(request_conversation, thread_id)?;

            let recent_conversations = GetRecentConversationsQuery::new(thread_id, 50)
                .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
                .await?;

            let already_resolved = recent_conversations.into_iter().any(|conversation| {
                matches!(
                    conversation.content,
                    ConversationContent::ApprovalResponse {
                        request_message_id: resolved_request_id,
                        ..
                    } if resolved_request_id == Some(request_message_id)
                )
            });
            if already_resolved {
                return Err(AppError::BadRequest(
                    "This approval request has already been resolved".to_string(),
                ));
            }

            let requested_tools = pending_request
                .tools
                .into_iter()
                .map(|tool| (tool.tool_name, tool.tool_id))
                .collect::<HashMap<_, _>>();

            let mut seen = HashSet::new();
            let mut decisions = Vec::new();

            for approval in &approval_response.approvals {
                if approval.tool_name.trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "Approval response tool names must be non-empty".to_string(),
                    ));
                }
                if !seen.insert(approval.tool_name.clone()) {
                    return Err(AppError::BadRequest(format!(
                        "Approval response contains duplicate tool '{}'",
                        approval.tool_name
                    )));
                }

                requested_tools
                    .get(&approval.tool_name)
                    .copied()
                    .ok_or_else(|| {
                        AppError::BadRequest(format!(
                            "Tool '{}' was not part of the pending approval request",
                            approval.tool_name
                        ))
                    })?;

                decisions.push(ToolApprovalDecision {
                    tool_name: approval.tool_name.clone(),
                    mode: approval.mode,
                });
            }

            let conversation_id = next_conversation_id(app_state)?;
            CreateConversationCommand::new(
                conversation_id,
                thread_id,
                ConversationContent::ApprovalResponse {
                    request_message_id: Some(request_message_id),
                    approvals: decisions,
                },
                ConversationMessageType::ApprovalResponse,
            )
            .execute_with_db(app_state.db_router.writer())
            .await?;

            let thread_event = enqueue_thread_event(
                app_state,
                &thread_state,
                thread_id,
                "approval_response_received",
                10,
                serde_json::to_value(models::thread_event::ApprovalResponseReceivedEventPayload {
                    conversation_id,
                    request_message_id,
                    approvals: approval_response
                        .approvals
                        .iter()
                        .cloned()
                        .map(|approval| models::ToolApprovalDecision {
                            tool_name: approval.tool_name,
                            mode: approval.mode,
                        })
                        .collect(),
                })?,
                Some(conversation_id),
                agent_id,
            )
            .await?;

            if thread_event.wake_disposition == ThreadEventWakeDisposition::Published {
                info!(
                    "Published approval_response execution for thread {} (conversation_id: {})",
                    thread_id, conversation_id
                );
            } else {
                info!(
                    "Queued approval_response event {} for busy thread {} (conversation_id: {})",
                    thread_event.event.id, thread_id, conversation_id
                );
            }

            Ok(queued_execution_response(Some(conversation_id)))
        }
        (None, None, Some(_)) => {
            enqueue_thread_control_event_if_needed(
                app_state,
                &thread_state,
                has_active_execution,
                models::thread_event::event_type::CONTROL_STOP,
            )
            .await?;

            let update_context_command =
                UpdateAgentThreadStateCommand::new(thread_id, deployment_id)
                    .with_status(AgentThreadStatus::Interrupted)
                    .mark_status_as_cancellation();
            let update_deps = deps::from_app(app_state).db().nats().id();
            update_context_command
                .execute_with_deps(&update_deps)
                .await?;

            Ok(ExecuteAgentResponse {
                status: "cancelled".to_string(),
                conversation_id: None,
            })
        }
        _ => Err(AppError::BadRequest(
            EXECUTION_VARIANT_VALIDATION_ERROR.to_string(),
        )),
    }
}
