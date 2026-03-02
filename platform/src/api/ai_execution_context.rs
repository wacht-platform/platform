use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::{ConsoleDeployment, RequireDeployment};
use axum::extract::{Json, Path, Query, State};

use commands::agent_execution::{
    PublishAgentExecutionCommand, SignalAgentExecutionCancellationCommand, UploadFilesToS3Command,
};
use commands::{
    Command, CreateConversationCommand, CreateExecutionContextCommand,
    EnsurePulseUsageAllowedForDeploymentCommand, UpdateExecutionContextCommand,
};
use common::error::AppError;
use common::state::AppState;
use dto::json::UpdateExecutionContextRequest;
use dto::json::deployment::{
    CreateExecutionContextRequest, ExecuteAgentRequest, ExecuteAgentResponse,
};
use models::plan_features::PlanFeature;
use models::{AgentExecutionContext, ExecutionContextStatus};
use queries::{
    GetDeploymentAiSettingsQuery,
    GetExecutionContextQuery, ListExecutionContextsQuery,
    plan_access::CheckDeploymentFeatureAccessQuery, Query as QueryTrait,
};
use serde::Deserialize;
use tracing::{error, info};

#[derive(Debug, Deserialize)]
pub struct ListExecutionContextsParams {
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub status: Option<String>,
    pub context_group: Option<String>,
}

pub async fn create_execution_context(
    State(app_state): State<AppState>,
    ConsoleDeployment(deployment_id): ConsoleDeployment,
    Json(request): Json<CreateExecutionContextRequest>,
) -> ApiResult<AgentExecutionContext> {
    let mut command = CreateExecutionContextCommand::new(deployment_id);

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
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn create_execution_context_backend(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateExecutionContextRequest>,
) -> ApiResult<AgentExecutionContext> {
    let mut command = CreateExecutionContextCommand::new(deployment_id);

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
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_execution_contexts(
    State(app_state): State<AppState>,
    ConsoleDeployment(deployment_id): ConsoleDeployment,
    Query(params): Query<ListExecutionContextsParams>,
) -> ApiResult<PaginatedResponse<AgentExecutionContext>> {
    let limit = params.limit.unwrap_or(50);

    let mut query = ListExecutionContextsQuery::new(deployment_id)
        .with_limit(limit + 1)
        .with_offset(params.offset.unwrap_or(0));

    if let Some(status) = params.status {
        query = query.with_status_filter(status);
    }

    if let Some(context_group) = params.context_group {
        query = query.with_context_group_filter(context_group);
    }

    let contexts = query.execute(&app_state).await?;

    let has_more = contexts.len() > limit as usize;
    let contexts = if has_more {
        contexts[..limit as usize].to_vec()
    } else {
        contexts
    };

    Ok(PaginatedResponse {
        data: contexts,
        has_more,
        limit: Some(limit as i32),
        offset: Some(params.offset.unwrap_or(0) as i32),
    }
    .into())
}

pub async fn get_execution_contexts_backend(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListExecutionContextsParams>,
) -> ApiResult<PaginatedResponse<AgentExecutionContext>> {
    let limit = params.limit.unwrap_or(50);

    let mut query = ListExecutionContextsQuery::new(deployment_id)
        .with_limit(limit + 1)
        .with_offset(params.offset.unwrap_or(0));

    if let Some(status) = params.status {
        query = query.with_status_filter(status);
    }

    if let Some(context_group) = params.context_group {
        query = query.with_context_group_filter(context_group);
    }

    let contexts = query.execute(&app_state).await?;

    let has_more = contexts.len() > limit as usize;
    let contexts = if has_more {
        contexts[..limit as usize].to_vec()
    } else {
        contexts
    };

    Ok(PaginatedResponse {
        data: contexts,
        has_more,
        limit: Some(limit as i32),
        offset: Some(params.offset.unwrap_or(0) as i32),
    }
    .into())
}
#[derive(Deserialize)]
pub struct ContextIdParam {
    pub context_id: i64,
}

pub async fn update_execution_context(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ContextIdParam>,
    Json(request): Json<UpdateExecutionContextRequest>,
) -> ApiResult<AgentExecutionContext> {
    let mut command = UpdateExecutionContextCommand::new(params.context_id, deployment_id);

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
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

#[derive(Deserialize)]
pub struct ExecuteParams {
    pub context_id: i64,
}

pub async fn execute_agent_async(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ExecuteParams>,
    Json(request): Json<ExecuteAgentRequest>,
) -> ApiResult<ExecuteAgentResponse> {
    use dto::json::deployment::ExecuteAgentRequestType;
    use models::{ConversationContent, ConversationMessageType};

    let context_id = params.context_id;

    let context = GetExecutionContextQuery::new(context_id, deployment_id)
        .execute(&app_state)
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
            "Exactly one execution_type variant must be provided".to_string(),
        )
        .into());
    }

    // We allow agent creation/config for all plans, but actual execution requires AI feature access.
    if cancel.is_none() {
        let has_ai_access = CheckDeploymentFeatureAccessQuery::new(
            deployment_id,
            PlanFeature::AiAgents,
        )
        .execute(&app_state)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to check AI feature access: {}", e)))?;

        if !has_ai_access {
            return Err(AppError::Forbidden("AI agent usage requires Growth plan".to_string()).into());
        }
    }

    let has_custom_gemini_key = if cancel.is_none() {
        GetDeploymentAiSettingsQuery::new(deployment_id)
            .execute(&app_state)
            .await?
            .and_then(|s| s.gemini_api_key)
            .is_some()
    } else {
        false
    };

    // Note: We pass agent_name through to the worker, which resolves the target agent.
    let agent_name = request.agent_name.clone();

    match (
        new_message,
        user_input_response,
        platform_function_result,
        cancel,
    ) {
        (Some(new_message), None, None, None) => {
            if !has_custom_gemini_key {
                EnsurePulseUsageAllowedForDeploymentCommand::new(deployment_id)
                    .execute(&app_state)
                    .await?;
            }

            if new_message.message.trim().is_empty()
                && new_message
                    .files
                    .as_ref()
                    .map_or(true, |files| files.is_empty())
            {
                return Err(AppError::BadRequest("Message or files required".to_string()).into());
            }

            let model_files =
                match UploadFilesToS3Command::new(deployment_id, context_id, new_message.files)
                    .execute(&app_state)
                    .await
                {
                    Ok(files) => files,
                    Err(e) => {
                        error!("Failed to upload files: {}", e);
                        None
                    }
                };

            let conversation_id = app_state.sf.next_id().map_err(|e| {
                AppError::Internal(format!("Failed to generate conversation ID: {}", e))
            })? as i64;

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
            .execute(&app_state)
            .await?;

            if has_running_execution {
                SignalAgentExecutionCancellationCommand::new(context_id)
                    .execute(&app_state)
                    .await?;
            }

            PublishAgentExecutionCommand::new_message(
                deployment_id,
                context_id,
                None,
                agent_name.clone(),
                conversation_id,
            )
            .execute(&app_state)
            .await?;

            info!(
                "Published new_message execution for context {} (conversation_id: {})",
                context_id, conversation_id
            );

            Ok(ExecuteAgentResponse {
                status: "queued".to_string(),
                conversation_id: Some(conversation_id.to_string()),
            }
            .into())
        }

        (None, Some(user_input_response), None, None) => {
            if !has_custom_gemini_key {
                EnsurePulseUsageAllowedForDeploymentCommand::new(deployment_id)
                    .execute(&app_state)
                    .await?;
            }

            if user_input_response.message.trim().is_empty() {
                return Err(AppError::BadRequest("Message is required".to_string()).into());
            }

            let conversation_id = app_state.sf.next_id().map_err(|e| {
                AppError::Internal(format!("Failed to generate conversation ID: {}", e))
            })? as i64;

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
            .execute(&app_state)
            .await?;

            if has_running_execution {
                SignalAgentExecutionCancellationCommand::new(context_id)
                    .execute(&app_state)
                    .await?;
            }

            PublishAgentExecutionCommand::user_input_response(
                deployment_id,
                context_id,
                None,
                agent_name.clone(),
                conversation_id,
            )
            .execute(&app_state)
            .await?;

            info!(
                "Published user_input_response execution for context {} (conversation_id: {})",
                context_id, conversation_id
            );

            Ok(ExecuteAgentResponse {
                status: "queued".to_string(),
                conversation_id: Some(conversation_id.to_string()),
            }
            .into())
        }

        (None, None, Some(platform_function_result), None) => {
            if platform_function_result.execution_id.trim().is_empty() {
                return Err(AppError::BadRequest("Execution ID is required".to_string()).into());
            }

            PublishAgentExecutionCommand::platform_function_result(
                deployment_id,
                context_id,
                None,
                agent_name.clone(),
                platform_function_result.execution_id.clone(),
                platform_function_result.result,
            )
            .execute(&app_state)
            .await?;

            info!(
                "Published platform_function_result execution for context {} (execution_id: {})",
                context_id, platform_function_result.execution_id
            );

            Ok(ExecuteAgentResponse {
                status: "queued".to_string(),
                conversation_id: None,
            }
            .into())
        }

        (None, None, None, Some(_)) => {
            if has_running_execution {
                SignalAgentExecutionCancellationCommand::new(context_id)
                    .execute(&app_state)
                    .await?;
            }

            UpdateExecutionContextCommand::new(context_id, deployment_id)
                .with_status(ExecutionContextStatus::Failed)
                .execute(&app_state)
                .await?;

            Ok(ExecuteAgentResponse {
                status: "cancelled".to_string(),
                conversation_id: None,
            }
            .into())
        }
        _ => Err(AppError::BadRequest(
            "Exactly one execution_type variant must be provided".to_string(),
        )
        .into()),
    }
}
