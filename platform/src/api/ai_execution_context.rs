use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::{ConsoleDeployment, RequireDeployment};
use axum::extract::{Json, Path, Query, State};

use agent_engine::{AgentHandler, ExecutionRequest};
use commands::{Command, CreateExecutionContextCommand, UpdateExecutionContextCommand};
use common::error::AppError;
use common::state::AppState;
use dto::json::UpdateExecutionContextRequest;
use dto::json::deployment::{
    CreateExecutionContextRequest, ExecuteAgentRequest, ExecuteAgentResponse,
};
use models::{AgentExecutionContext, ExecutionContextStatus};
use queries::{
    GetAiAgentByNameWithFeatures, GetExecutionContextQuery, ListExecutionContextsQuery,
    Query as QueryTrait,
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
    let context_id = params.context_id;
    GetExecutionContextQuery::new(context_id, deployment_id)
        .execute(&app_state)
        .await?;

    let agent = GetAiAgentByNameWithFeatures::new(deployment_id, request.agent_name.clone())
        .execute(&app_state)
        .await?;

    let execution_id = app_state
        .sf
        .next_id()
        .map_err(|e| AppError::Internal(format!("Failed to generate execution ID: {}", e)))?
        as i64;

    let execution_request = ExecutionRequest {
        agent,
        user_message: Some(request.message),
        user_images: request.images,
        context_id,
        platform_function_result: request.platform_function_result,
    };

    tokio::spawn(async move {
        info!(
            "Starting background execution {} for context {} in deployment {}",
            execution_id, context_id, deployment_id
        );

        let handler = AgentHandler::new(app_state);
        match handler.execute_agent_streaming(execution_request).await {
            Ok(_) => {
                info!(
                    "Successfully completed execution {} for context {}",
                    execution_id, context_id
                );
            }
            Err(e) => {
                error!("Failed to execute agent for context {}: {}", context_id, e);
            }
        }
    });

    info!(
        "Started async agent execution {} for context {} in deployment {}",
        execution_id, context_id, deployment_id
    );

    Ok(ExecuteAgentResponse {
        execution_id,
        status: "running".to_string(),
    }
    .into())
}
