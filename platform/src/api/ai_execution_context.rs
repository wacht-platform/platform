use crate::application::{
    ai_execution_context as ai_execution_context_app,
    response::{ApiResult, PaginatedResponse},
};
use crate::middleware::{ConsoleDeployment, RequireDeployment};
use axum::extract::{Json, Path, Query, State};

use common::state::AppState;
use dto::json::UpdateExecutionContextRequest;
use dto::json::deployment::{
    CreateExecutionContextRequest, ExecuteAgentRequest, ExecuteAgentResponse,
};
use models::AgentExecutionContext;
use serde::Deserialize;

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
    let context = ai_execution_context_app::create_execution_context(
        &app_state,
        deployment_id,
        request,
    )
    .await?;
    Ok(context.into())
}

pub async fn create_execution_context_backend(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateExecutionContextRequest>,
) -> ApiResult<AgentExecutionContext> {
    let context = ai_execution_context_app::create_execution_context(
        &app_state,
        deployment_id,
        request,
    )
    .await?;
    Ok(context.into())
}

pub async fn get_execution_contexts(
    State(app_state): State<AppState>,
    ConsoleDeployment(deployment_id): ConsoleDeployment,
    Query(params): Query<ListExecutionContextsParams>,
) -> ApiResult<PaginatedResponse<AgentExecutionContext>> {
    let contexts = ai_execution_context_app::get_execution_contexts(
        &app_state,
        deployment_id,
        params.limit.unwrap_or(50),
        params.offset.unwrap_or(0),
        params.status,
        params.context_group,
    )
    .await?;
    Ok(contexts.into())
}

pub async fn get_execution_contexts_backend(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(params): Query<ListExecutionContextsParams>,
) -> ApiResult<PaginatedResponse<AgentExecutionContext>> {
    let contexts = ai_execution_context_app::get_execution_contexts(
        &app_state,
        deployment_id,
        params.limit.unwrap_or(50),
        params.offset.unwrap_or(0),
        params.status,
        params.context_group,
    )
    .await?;
    Ok(contexts.into())
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
    let context = ai_execution_context_app::update_execution_context(
        &app_state,
        deployment_id,
        params.context_id,
        request,
    )
    .await?;
    Ok(context.into())
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
    let response = ai_execution_context_app::execute_agent_async(
        &app_state,
        deployment_id,
        params.context_id,
        request,
    )
    .await?;
    Ok(response.into())
}
