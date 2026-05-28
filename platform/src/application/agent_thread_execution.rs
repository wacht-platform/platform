use commands::agent_execution::UploadFilesToS3Command;
use commands::{
    AdvanceThreadExecutionTokenCommand, ClearThreadPendingQuestionCommand,
    CreateConversationCommand, EnsurePulseUsageAllowedForDeploymentCommand,
    UpdateAgentThreadStateCommand,
    event_log::{self, EnqueueThreadWorkEvent},
};
use common::ReadConsistency;
use common::ResultExt;
use common::error::AppError;
use dto::json::ask_user::{AnswerSubmission, validate_answers};
use dto::json::deployment::{ExecuteAgentRequest, ExecuteAgentResponse};
use models::{
    AgentThreadStatus, ConversationContent, ConversationMessageType, RequestedToolApprovalState,
    ToolApprovalDecision, ToolApprovalRequestState,
};
use queries::{
    GetAgentThreadStateQuery, GetConversationByIdQuery, GetDeploymentAiSettingsQuery,
    GetLatestPendingClarificationOnThreadQuery, GetRecentConversationsQuery,
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
        .map_err_internal("Failed to generate conversation ID")? as i64)
}

fn parse_pending_approval_request(
    conversation: models::ConversationRecord,
    thread_id: i64,
) -> Result<ToolApprovalRequestState, AppError> {
    if conversation.thread_id != Some(thread_id) {
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

async fn enqueue_thread_work(
    app_state: &AppState,
    thread_state: &models::AgentThreadState,
    thread_id: i64,
    priority: i32,
    agent_id: Option<i64>,
    execution_type: dto::json::AgentExecutionType,
) -> Result<i64, AppError> {
    let event_log_id = app_state.sf.next_id()? as i64;
    let (event_type, conversation_id, approval_request_message_id): (
        &str,
        Option<i64>,
        Option<String>,
    ) = match &execution_type {
        dto::json::AgentExecutionType::NewMessage { conversation_id } => (
            models::thread_event::event_type::USER_MESSAGE_RECEIVED,
            conversation_id.parse::<i64>().ok(),
            None,
        ),
        dto::json::AgentExecutionType::ApprovalResponse {
            request_message_id, ..
        } => (
            models::thread_event::event_type::APPROVAL_RESPONSE_RECEIVED,
            None,
            Some(request_message_id.clone()),
        ),
    };

    let request = dto::json::AgentExecutionRequest {
        deployment_id: thread_state.deployment_id.to_string(),
        thread_id: thread_id.to_string(),
        agent_id: agent_id.map(|id| id.to_string()),
        execution_type,
    };

    let execution_payload = serde_json::to_value(&request).map_err(|err| {
        AppError::Internal(format!(
            "Failed to serialize agent execution request: {err}"
        ))
    })?;

    let idempotency_key = if let Some(cid) = conversation_id {
        format!("{event_type}_{thread_id}_{cid}")
    } else if let Some(rmid) = approval_request_message_id.as_deref() {
        format!("{event_type}_{thread_id}_{rmid}")
    } else {
        format!("{event_type}_{thread_id}_{event_log_id}")
    };

    EnqueueThreadWorkEvent {
        event_log_id,
        deployment_id: thread_state.deployment_id,
        thread_id,
        event_type: event_type.to_string(),
        priority,
        agent_id,
        conversation_id,
        idempotency_key,
        execution_payload,
    }
    .execute(app_state.db_router.writer())
    .await?;

    event_log::nudge_dispatcher(&app_state.nats_client).await;

    Ok(event_log_id)
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

pub async fn execute_agent_async(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    request: ExecuteAgentRequest,
) -> Result<ExecuteAgentResponse, AppError> {
    use dto::json::deployment::ExecuteAgentRequestType;

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
            if thread_state.thread_purpose == models::agent_thread::purpose::CONVERSATION
                && matches!(thread_state.status, AgentThreadStatus::Running)
            {
                AdvanceThreadExecutionTokenCommand::new(thread_id)
                    .execute_with_deps(&deps::from_app(app_state).nats().id())
                    .await?;
            }
            if matches!(thread_state.status, AgentThreadStatus::WaitingForInput) {
                let mut update = UpdateAgentThreadStateCommand::new(thread_id, deployment_id)
                    .with_status(AgentThreadStatus::Interrupted);
                if let Some(mut execution_state) = thread_state.execution_state.clone() {
                    execution_state.pending_approval_request = None;
                    execution_state.pending_question = None;
                    update = update.with_execution_state(execution_state);
                }
                update
                    .execute_with_deps(&deps::from_app(app_state).db().nats().id())
                    .await?;
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

            let event_log_id = enqueue_thread_work(
                app_state,
                &thread_state,
                thread_id,
                70,
                agent_id,
                dto::json::AgentExecutionType::NewMessage {
                    conversation_id: conversation_id.to_string(),
                },
            )
            .await?;

            info!(
                event_log_id,
                thread_id, conversation_id, "queued user_message_received"
            );

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

            let conv_query = GetConversationByIdQuery::new(request_message_id);
            let recent_query = GetRecentConversationsQuery::new(thread_id, 50);
            let (request_conversation, recent_conversations) = tokio::try_join!(
                conv_query.execute_with_db(app_state.db_router.reader(ReadConsistency::Strong)),
                recent_query.execute_with_db(app_state.db_router.reader(ReadConsistency::Strong)),
            )?;

            let pending_request = parse_pending_approval_request(request_conversation, thread_id)?;

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

            let event_log_id = enqueue_thread_work(
                app_state,
                &thread_state,
                thread_id,
                10,
                agent_id,
                dto::json::AgentExecutionType::ApprovalResponse {
                    request_message_id: request_message_id.to_string(),
                    approvals: approval_response.approvals.clone(),
                },
            )
            .await?;

            info!(
                event_log_id,
                thread_id, conversation_id, "queued approval_response_received"
            );

            Ok(queued_execution_response(Some(conversation_id)))
        }
        (None, None, Some(_)) => {
            let _ = has_active_execution;
            if matches!(thread_state.status, AgentThreadStatus::Running) {
                AdvanceThreadExecutionTokenCommand::new(thread_id)
                    .execute_with_deps(&deps::from_app(app_state).nats().id())
                    .await?;
            }
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

pub async fn answer_thread_question(
    app_state: &AppState,
    deployment_id: i64,
    thread_id: i64,
    submission: AnswerSubmission,
) -> Result<ExecuteAgentResponse, AppError> {
    GetAgentThreadStateQuery::new(thread_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    let pending_row = GetLatestPendingClarificationOnThreadQuery::new(thread_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?
        .ok_or_else(|| {
            AppError::BadRequest("no pending clarification on this thread".to_string())
        })?;

    let request_message_id = pending_row.id;

    let content: ConversationContent = serde_json::from_value(pending_row.content)
        .map_err(|e| AppError::Internal(format!("malformed clarification_request content: {e}")))?;
    let (questions_value, context) = match content {
        ConversationContent::ClarificationRequest { questions, context } => (questions, context),
        _ => {
            return Err(AppError::Internal(
                "latest clarification_request row had non-clarification content".to_string(),
            ));
        }
    };
    let questions: Vec<models::Question> = serde_json::from_value(questions_value)
        .map_err(|e| AppError::BadRequest(format!("malformed questions in pending row: {e}")))?;

    let pending = models::PendingQuestion {
        questions,
        context,
        asked_at: chrono::Utc::now(),
        asked_by_thread_id: thread_id,
        asked_by_assignment_id: None,
    };
    validate_answers(&pending, &submission).map_err(AppError::BadRequest)?;

    let freeform_text = submission.freeform_trimmed();
    let answers_json =
        serde_json::to_value(&submission.answers).map_err_internal("serialize answers")?;
    let conv_id = next_conversation_id(app_state)?;
    let response_content = ConversationContent::ClarificationResponse {
        request_message_id: Some(request_message_id),
        answers: answers_json,
        freeform_text,
    };

    let mut tx = app_state.db_router.writer().begin().await?;

    CreateConversationCommand::new(
        conv_id,
        thread_id,
        response_content,
        ConversationMessageType::ClarificationResponse,
    )
    .execute_with_db(&mut *tx)
    .await?;

    ClearThreadPendingQuestionCommand { thread_id }
        .execute_with_db(&mut *tx)
        .await?;

    let event_log_id = app_state.sf.next_id()? as i64;
    let execution_type = dto::json::AgentExecutionType::NewMessage {
        conversation_id: conv_id.to_string(),
    };
    let execution_payload = serde_json::to_value(&dto::json::AgentExecutionRequest {
        deployment_id: deployment_id.to_string(),
        thread_id: thread_id.to_string(),
        agent_id: None,
        execution_type,
    })
    .map_err_internal("serialize agent execution request")?;

    EnqueueThreadWorkEvent {
        event_log_id,
        deployment_id,
        thread_id,
        event_type: models::thread_event::event_type::USER_MESSAGE_RECEIVED.to_string(),
        priority: 70,
        agent_id: None,
        conversation_id: Some(conv_id),
        idempotency_key: format!("user_message_received_{thread_id}_{conv_id}"),
        execution_payload,
    }
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    event_log::nudge_dispatcher(&app_state.nats_client).await;

    Ok(queued_execution_response(Some(conv_id)))
}
