use axum::extract::{Json, Path, Query, State};

use crate::{
    application::{
        HttpState,
        response::{ApiResult, PaginatedResponse},
    },
    core::{
        commands::{Command, CreateAiToolCommand, DeleteAiToolCommand, UpdateAiToolCommand},
        dto::{
            json::deployment::{CreateToolRequest, UpdateToolRequest},
            query::deployment::GetToolsQuery,
        },
        models::{AiTool, AiToolType, AiToolWithDetails},
        queries::{GetAiToolByIdQuery, GetAiToolsQuery, Query as QueryTrait},
    },
};

pub async fn get_ai_tools(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    Query(query): Query<GetToolsQuery>,
) -> ApiResult<PaginatedResponse<AiToolWithDetails>> {
    let limit = query.limit.unwrap_or(50) as u32;

    let tools = GetAiToolsQuery::new(deployment_id)
        .with_limit(Some(limit + 1))
        .with_offset(query.offset.map(|o| o as u32))
        .with_search(query.search)
        .execute(&app_state)
        .await?;

    let has_more = tools.len() > limit as usize;
    let tools = if has_more {
        tools[..limit as usize].to_vec()
    } else {
        tools
    };

    Ok(PaginatedResponse {
        data: tools,
        has_more,
    }
    .into())
}

pub async fn create_ai_tool(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    Json(request): Json<CreateToolRequest>,
) -> ApiResult<AiTool> {
    let tool_type = AiToolType::from(request.tool_type);

    CreateAiToolCommand::new(
        deployment_id,
        request.name,
        request.description,
        tool_type,
        request.configuration,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn get_ai_tool_by_id(
    State(app_state): State<HttpState>,
    Path((deployment_id, tool_id)): Path<(i64, i64)>,
) -> ApiResult<AiToolWithDetails> {
    GetAiToolByIdQuery::new(deployment_id, tool_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_ai_tool(
    State(app_state): State<HttpState>,
    Path((deployment_id, tool_id)): Path<(i64, i64)>,
    Json(request): Json<UpdateToolRequest>,
) -> ApiResult<AiTool> {
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

    command
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_ai_tool(
    State(app_state): State<HttpState>,
    Path((deployment_id, tool_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    DeleteAiToolCommand::new(deployment_id, tool_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
