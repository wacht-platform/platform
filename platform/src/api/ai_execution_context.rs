use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::{ConsoleDeployment, RequireDeployment};
use axum::extract::{Json, Path, Query, State};

use commands::{Command, CreateExecutionContextCommand};
use common::error::AppError;
use common::state::AppState;
use dto::json::deployment::{CreateExecutionContextRequest, ExecuteAgentRequest, ExecuteAgentResponse};
use models::AgentExecutionContext;
use queries::{GetAiAgentByNameWithFeatures, GetExecutionContextQuery, ListExecutionContextsQuery, Query as QueryTrait};
use serde::Deserialize;
use serde_json::json;
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
    
    // Verify context exists and belongs to deployment
    GetExecutionContextQuery::new(context_id, deployment_id)
        .execute(&app_state)
        .await?;
    
    // Verify agent exists
    GetAiAgentByNameWithFeatures::new(deployment_id, request.agent_name.clone())
        .execute(&app_state)
        .await?;
    
    // Generate execution ID
    let execution_id = app_state.sf.next_id()
        .map_err(|e| AppError::Internal(format!("Failed to generate execution ID: {}", e)))? as i64;
    
    // Publish to NATS for background processing
    let subject = format!("agent.execute.{}.{}", deployment_id, context_id);
    let payload = json!({
        "execution_id": execution_id,
        "deployment_id": deployment_id,
        "context_id": context_id,
        "agent_name": request.agent_name,
        "message": request.message,
        "platform_function_result": request.platform_function_result,
    });
    
    let payload_bytes = serde_json::to_vec(&payload)
        .map_err(|e| AppError::Internal(format!("Failed to serialize payload: {}", e)))?;
    
    app_state
        .nats_jetstream
        .publish(subject, payload_bytes.into())
        .await
        .map_err(|e| {
            error!("Failed to publish execution request: {}", e);
            AppError::Internal("Failed to queue execution".to_string())
        })?
        .await
        .map_err(|e| {
            error!("Failed to publish execution request: {}", e);
            AppError::Internal("Failed to queue execution".to_string())
        })?;
    
    info!(
        "Queued agent execution {} for context {} in deployment {}",
        execution_id, context_id, deployment_id
    );
    
    Ok(ExecuteAgentResponse {
        execution_id,
        status: "queued".to_string(),
    }
    .into())
}
