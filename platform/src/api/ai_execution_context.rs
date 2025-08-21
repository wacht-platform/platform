use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::ConsoleDeployment;
use axum::extract::{Json, Path, Query, State};

use commands::{Command, CreateExecutionContextCommand};
use common::state::AppState;
use dto::json::deployment::CreateExecutionContextRequest;
use models::AgentExecutionContext;
use queries::{GetExecutionContextQuery, ListExecutionContextsQuery, Query as QueryTrait};
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
    }
    .into())
}

pub async fn get_execution_context_by_id(
    State(app_state): State<AppState>,
    ConsoleDeployment(deployment_id): ConsoleDeployment,
    Path(context_id): Path<i64>,
) -> ApiResult<AgentExecutionContext> {
    let query = GetExecutionContextQuery::new(context_id, deployment_id);

    query
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
