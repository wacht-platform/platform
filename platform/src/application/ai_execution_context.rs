use commands::agent_execution::{
    PublishAgentExecutionCommand, SignalAgentExecutionCancellationCommand, UploadFilesToS3Command,
};
use commands::{
    CreateConversationCommand, CreateExecutionContextCommand,
    EnsurePulseUsageAllowedForDeploymentCommand, UpdateExecutionContextCommand,
    UpdateExecutionContextStateCommand,
};
use common::ReadConsistency;
use common::error::AppError;
use dto::json::UpdateExecutionContextRequest;
use dto::json::deployment::{
    CreateExecutionContextRequest, ExecuteAgentRequest, ExecuteAgentResponse,
};
use models::plan_features::PlanFeature;
use models::{AgentExecutionContext, ExecutionContextStatus};
use queries::{
    GetDeploymentAiSettingsQuery, GetExecutionContextQuery, ListExecutionContextsQuery,
    plan_access::CheckDeploymentFeatureAccessQuery,
};
use tracing::{error, info};

use crate::{
    api::pagination::paginate_results,
    application::{AppState, response::PaginatedResponse},
};
use common::deps;

const EXECUTION_VARIANT_VALIDATION_ERROR: &str =
    "Exactly one execution_type variant must be provided";

fn build_create_execution_context_command(
    context_id: i64,
    deployment_id: i64,
    request: CreateExecutionContextRequest,
) -> CreateExecutionContextCommand {
    let mut command = CreateExecutionContextCommand::new(context_id, deployment_id);

    if let Some(title) = request.title {
        command = command.with_title(title);
    }
    if let Some(system_instructions) = request.system_instructions {
        command = command.with_system_instructions(system_instructions);
    }
    if let Some(context_group) = request.context_group {
        command = command.with_context_group(context_group);
    }

    command
}

fn build_update_execution_context_command(
    context_id: i64,
    deployment_id: i64,
    request: UpdateExecutionContextRequest,
) -> UpdateExecutionContextCommand {
    let mut command = UpdateExecutionContextCommand::new(context_id, deployment_id);

    if let Some(title) = request.title {
        command = command.with_title(title);
    }
    if let Some(system_instructions) = request.system_instructions {
        command = command.with_system_instructions(system_instructions);
    }
    if let Some(context_group) = request.context_group {
        command = command.with_context_group(context_group);
    }
    if let Some(status_str) = request.status {
        use std::str::FromStr;
        if let Ok(status) = ExecutionContextStatus::from_str(&status_str) {
            command = command.with_status(status);
        }
    }

    command
}

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

async fn publish_execution_command(
    app_state: &AppState,
    command: PublishAgentExecutionCommand,
) -> Result<(), AppError> {
    let execution_deps = deps::from_app(app_state).nats().id();
    command.execute_with_deps(&execution_deps).await
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

async fn cancel_running_execution_if_needed(
    app_state: &AppState,
    context_id: i64,
    has_running_execution: bool,
) -> Result<(), AppError> {
    if has_running_execution {
        SignalAgentExecutionCancellationCommand::new(context_id)
            .execute_with_deps(&deps::from_app(app_state).nats().id())
            .await?;
    }
    Ok(())
}

pub async fn create_execution_context(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateExecutionContextRequest,
) -> Result<AgentExecutionContext, AppError> {
    let context_id = app_state.sf.next_id()? as i64;
    build_create_execution_context_command(context_id, deployment_id, request)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn get_execution_contexts(
    app_state: &AppState,
    deployment_id: i64,
    limit: u32,
    offset: u32,
    status: Option<String>,
    context_group: Option<String>,
) -> Result<PaginatedResponse<AgentExecutionContext>, AppError> {
    let mut query = ListExecutionContextsQuery::new(deployment_id)
        .with_limit(limit + 1)
        .with_offset(offset);

    if let Some(status) = status {
        query = query.with_status_filter(status);
    }
    if let Some(context_group) = context_group {
        query = query.with_context_group_filter(context_group);
    }

    let contexts = query
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;
    Ok(paginate_results(
        contexts,
        limit as i32,
        Some(offset as i64),
    ))
}

pub async fn update_execution_context(
    app_state: &AppState,
    deployment_id: i64,
    context_id: i64,
    request: UpdateExecutionContextRequest,
) -> Result<AgentExecutionContext, AppError> {
    build_update_execution_context_command(context_id, deployment_id, request)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn execute_agent_async(
    app_state: &AppState,
    deployment_id: i64,
    context_id: i64,
    request: ExecuteAgentRequest,
) -> Result<ExecuteAgentResponse, AppError> {
    use dto::json::deployment::ExecuteAgentRequestType;
    use models::{ConversationContent, ConversationMessageType};

    let context = GetExecutionContextQuery::new(context_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;
    let has_running_execution = matches!(context.status, ExecutionContextStatus::Running);

    let ExecuteAgentRequestType {
        new_message,
        user_input_response,
        platform_function_result,
        cancel,
    } = request.execution_type;

    let execution_variants = [
        new_message.is_some(),
        user_input_response.is_some(),
        platform_function_result.is_some(),
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

    let agent_name = request.agent_name.clone();

    match (
        new_message,
        user_input_response,
        platform_function_result,
        cancel,
    ) {
        (Some(new_message), None, None, None) => {
            ensure_pulse_usage_if_needed(app_state, deployment_id, has_custom_gemini_key).await?;

            if new_message.message.trim().is_empty()
                && new_message
                    .files
                    .as_ref()
                    .is_none_or(|files| files.is_empty())
            {
                return Err(AppError::BadRequest(
                    "Message or files required".to_string(),
                ));
            }

            let upload_files_command =
                UploadFilesToS3Command::new(deployment_id, context_id, new_message.files);
            let model_files = match upload_files_command
                .execute_with_deps(&deps::from_app(app_state).id())
                .await
            {
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
                context_id,
                ConversationContent::UserMessage {
                    message,
                    sender_name: None,
                    files: model_files,
                },
                ConversationMessageType::UserMessage,
            )
            .execute_with_db(app_state.db_router.writer())
            .await?;

            cancel_running_execution_if_needed(app_state, context_id, has_running_execution)
                .await?;

            let publish_command = PublishAgentExecutionCommand::new_message(
                deployment_id,
                context_id,
                None,
                agent_name.clone(),
                conversation_id,
            );
            publish_execution_command(app_state, publish_command).await?;

            info!(
                "Published new_message execution for context {} (conversation_id: {})",
                context_id, conversation_id
            );

            Ok(queued_execution_response(Some(conversation_id)))
        }
        (None, Some(user_input_response), None, None) => {
            ensure_pulse_usage_if_needed(app_state, deployment_id, has_custom_gemini_key).await?;

            if user_input_response.message.trim().is_empty() {
                return Err(AppError::BadRequest("Message is required".to_string()));
            }

            let conversation_id = next_conversation_id(app_state)?;
            CreateConversationCommand::new(
                conversation_id,
                context_id,
                ConversationContent::UserMessage {
                    message: user_input_response.message,
                    sender_name: None,
                    files: None,
                },
                ConversationMessageType::UserMessage,
            )
            .execute_with_db(app_state.db_router.writer())
            .await?;

            cancel_running_execution_if_needed(app_state, context_id, has_running_execution)
                .await?;

            let publish_command = PublishAgentExecutionCommand::user_input_response(
                deployment_id,
                context_id,
                None,
                agent_name.clone(),
                conversation_id,
            );
            publish_execution_command(app_state, publish_command).await?;

            info!(
                "Published user_input_response execution for context {} (conversation_id: {})",
                context_id, conversation_id
            );

            Ok(queued_execution_response(Some(conversation_id)))
        }
        (None, None, Some(platform_function_result), None) => {
            if platform_function_result.execution_id.trim().is_empty() {
                return Err(AppError::BadRequest("Execution ID is required".to_string()));
            }

            let publish_command = PublishAgentExecutionCommand::platform_function_result(
                deployment_id,
                context_id,
                None,
                agent_name.clone(),
                platform_function_result.execution_id.clone(),
                platform_function_result.result,
            );
            publish_execution_command(app_state, publish_command).await?;

            info!(
                "Published platform_function_result execution for context {} (execution_id: {})",
                context_id, platform_function_result.execution_id
            );

            Ok(queued_execution_response(None))
        }
        (None, None, None, Some(_)) => {
            cancel_running_execution_if_needed(app_state, context_id, has_running_execution)
                .await?;

            let update_context_command =
                UpdateExecutionContextStateCommand::new(context_id, deployment_id)
                    .with_status(ExecutionContextStatus::Failed)
                    .mark_status_as_cancellation();
            let update_deps = deps::from_app(app_state).db().nats().id();
            update_context_command.execute_with_deps(&update_deps).await?;

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
