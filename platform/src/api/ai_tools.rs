use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::api::pagination::paginate_results;
use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{
    AttachToolToAgentCommand, Command, CreateAiToolCommand, DeleteAiToolCommand,
    DetachToolFromAgentCommand, UpdateAiToolCommand,
};
use dto::{
    json::deployment::{CreateToolRequest, UpdateToolRequest},
    query::deployment::GetToolsQuery,
};
use models::{AiTool, AiToolType, AiToolWithDetails};
use queries::{GetAgentToolsQuery, GetAiToolByIdQuery, GetAiToolsQuery, Query as QueryTrait};

// Unified parameter extraction for AI tool routes
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
    let limit = query.limit.unwrap_or(50) as i32;
    let query_limit = limit as u32;
    let offset = query.offset.map(|o| o as i64);

    let tools = GetAiToolsQuery::new(deployment_id)
        .with_limit(Some(query_limit + 1))
        .with_offset(offset.map(|o| o as u32))
        .with_search(query.search)
        .execute(&app_state)
        .await?;

    Ok(paginate_results(tools, limit, offset).into())
}

pub async fn create_ai_tool(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateToolRequest>,
) -> ApiResult<AiTool> {
    let tool_type = AiToolType::from(request.tool_type);

    let tool = CreateAiToolCommand::new(
        deployment_id,
        request.name,
        request.description,
        tool_type,
        request.configuration,
    )
    .execute(&app_state)
    .await?;
    Ok(tool.into())
}

pub async fn get_ai_tool_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ToolParams>,
) -> ApiResult<AiToolWithDetails> {
    let tool = GetAiToolByIdQuery::new(deployment_id, params.tool_id)
        .execute(&app_state)
        .await?;
    Ok(tool.into())
}

pub async fn get_agent_tools(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<PaginatedResponse<AiTool>> {
    let tools = GetAgentToolsQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await?;
    Ok(PaginatedResponse::from(tools).into())
}

pub async fn attach_tool_to_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentToolParams>,
) -> ApiResult<()> {
    AttachToolToAgentCommand::new(deployment_id, params.agent_id, params.tool_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn detach_tool_from_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentToolParams>,
) -> ApiResult<()> {
    DetachToolFromAgentCommand::new(deployment_id, params.agent_id, params.tool_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn update_ai_tool(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ToolParams>,
    Json(request): Json<UpdateToolRequest>,
) -> ApiResult<AiTool> {
    let mut command = UpdateAiToolCommand::new(deployment_id, params.tool_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(description) = request.description {
        command = command.with_description(Some(description));
    }
    if let Some(tool_type) = request.tool_type {
        command = command.with_tool_type(AiToolType::from(tool_type));
    }
    if let Some(configuration) = request.configuration {
        command = command.with_configuration(configuration);
    }

    let tool = command.execute(&app_state).await?;
    Ok(tool.into())
}

pub async fn delete_ai_tool(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<ToolParams>,
) -> ApiResult<()> {
    DeleteAiToolCommand::new(deployment_id, params.tool_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}
