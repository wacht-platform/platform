use commands::{
    AttachToolToAgentCommand, Command, CreateAiToolCommand, DeleteAiToolCommand,
    DetachToolFromAgentCommand, UpdateAiToolCommand,
};
use common::error::AppError;
use dto::{
    json::deployment::{CreateToolRequest, UpdateToolRequest},
    query::deployment::GetToolsQuery,
};
use models::{AiTool, AiToolType, AiToolWithDetails};
use queries::{GetAgentToolsQuery, GetAiToolByIdQuery, GetAiToolsQuery, Query as QueryTrait};

use crate::{
    api::pagination::paginate_results,
    application::{
        response::PaginatedResponse,
        AppState,
    },
};

pub async fn get_ai_tools(
    app_state: &AppState,
    deployment_id: i64,
    query: GetToolsQuery,
) -> Result<PaginatedResponse<AiToolWithDetails>, AppError> {
    let limit = query.limit.unwrap_or(50) as i32;
    let query_limit = limit as u32;
    let offset = query.offset.map(|o| o as i64);

    let tools = GetAiToolsQuery::new(deployment_id)
        .with_limit(Some(query_limit + 1))
        .with_offset(offset.map(|o| o as u32))
        .with_search(query.search)
        .execute(app_state)
        .await?;

    Ok(paginate_results(tools, limit, offset))
}

pub async fn create_ai_tool(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateToolRequest,
) -> Result<AiTool, AppError> {
    let tool_type = AiToolType::from(request.tool_type);
    CreateAiToolCommand::new(
        deployment_id,
        request.name,
        request.description,
        tool_type,
        request.configuration,
    )
    .execute(app_state)
    .await
}

pub async fn get_ai_tool_by_id(
    app_state: &AppState,
    deployment_id: i64,
    tool_id: i64,
) -> Result<AiToolWithDetails, AppError> {
    GetAiToolByIdQuery::new(deployment_id, tool_id)
        .execute(app_state)
        .await
}

pub async fn get_agent_tools(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<PaginatedResponse<AiTool>, AppError> {
    let tools = GetAgentToolsQuery::new(deployment_id, agent_id)
        .execute(app_state)
        .await?;
    Ok(PaginatedResponse::from(tools))
}

pub async fn attach_tool_to_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    tool_id: i64,
) -> Result<(), AppError> {
    AttachToolToAgentCommand::new(deployment_id, agent_id, tool_id)
        .execute(app_state)
        .await?;
    Ok(())
}

pub async fn detach_tool_from_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    tool_id: i64,
) -> Result<(), AppError> {
    DetachToolFromAgentCommand::new(deployment_id, agent_id, tool_id)
        .execute(app_state)
        .await?;
    Ok(())
}

pub async fn update_ai_tool(
    app_state: &AppState,
    deployment_id: i64,
    tool_id: i64,
    request: UpdateToolRequest,
) -> Result<AiTool, AppError> {
    let mut command = UpdateAiToolCommand::new(deployment_id, tool_id);

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

    command.execute(app_state).await
}

pub async fn delete_ai_tool(
    app_state: &AppState,
    deployment_id: i64,
    tool_id: i64,
) -> Result<(), AppError> {
    DeleteAiToolCommand::new(deployment_id, tool_id)
        .execute(app_state)
        .await?;
    Ok(())
}
