use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::application::{
    ai_tools as ai_tools_app,
    response::{ApiResult, PaginatedResponse},
};
use common::state::AppState;

use dto::{
    json::deployment::{CreateToolRequest, UpdateToolRequest},
    query::deployment::GetToolsQuery,
};
use models::{AiTool, AiToolWithDetails};

#[derive(Deserialize)]
pub struct ToolParams {
    pub tool_id: i64,
}

#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

#[derive(Deserialize)]
pub struct AgentToolParams {
    pub agent_id: i64,
    pub tool_id: i64,
}

pub async fn get_ai_tools(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<GetToolsQuery>,
) -> ApiResult<PaginatedResponse<AiToolWithDetails>> {
    let tools = ai_tools_app::get_ai_tools(&app_state, deployment_id, query).await?;
    Ok(tools.into())
}

pub async fn create_ai_tool(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateToolRequest>,
) -> ApiResult<AiTool> {
    let tool = ai_tools_app::create_ai_tool(&app_state, deployment_id, request).await?;
    Ok(tool.into())
}

pub async fn get_ai_tool_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ToolParams>,
) -> ApiResult<AiToolWithDetails> {
    let tool = ai_tools_app::get_ai_tool_by_id(&app_state, deployment_id, params.tool_id).await?;
    Ok(tool.into())
}

pub async fn get_agent_tools(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<PaginatedResponse<AiTool>> {
    let tools = ai_tools_app::get_agent_tools(&app_state, deployment_id, params.agent_id).await?;
    Ok(tools.into())
}

pub async fn attach_tool_to_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentToolParams>,
) -> ApiResult<()> {
    ai_tools_app::attach_tool_to_agent(&app_state, deployment_id, params.agent_id, params.tool_id)
        .await?;
    Ok(().into())
}

pub async fn detach_tool_from_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentToolParams>,
) -> ApiResult<()> {
    ai_tools_app::detach_tool_from_agent(
        &app_state,
        deployment_id,
        params.agent_id,
        params.tool_id,
    )
    .await?;
    Ok(().into())
}

pub async fn update_ai_tool(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ToolParams>,
    Json(request): Json<UpdateToolRequest>,
) -> ApiResult<AiTool> {
    let tool =
        ai_tools_app::update_ai_tool(&app_state, deployment_id, params.tool_id, request).await?;
    Ok(tool.into())
}

pub async fn delete_ai_tool(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ToolParams>,
) -> ApiResult<()> {
    ai_tools_app::delete_ai_tool(&app_state, deployment_id, params.tool_id).await?;
    Ok(().into())
}
