use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{Command, CreateAiAgentCommand, DeleteAiAgentCommand, UpdateAiAgentCommand};
use dto::{
    json::deployment::{CreateAgentRequest, UpdateAgentRequest},
    query::deployment::GetAgentsQuery,
};
use models::{AiAgent, AiAgentWithDetails};
use queries::{GetAiAgentByIdQuery, GetAiAgentsQuery, Query as QueryTrait};

// Unified parameter extraction for AI agent routes
#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

pub async fn get_ai_agents(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<GetAgentsQuery>,
) -> ApiResult<PaginatedResponse<AiAgentWithDetails>> {
    let limit = query.limit.unwrap_or(50) as u32;

    let agents = GetAiAgentsQuery::new(deployment_id)
        .with_limit(Some(limit + 1))
        .with_offset(query.offset.map(|o| o as u32))
        .with_search(query.search)
        .execute(&app_state)
        .await?;

    let has_more = agents.len() > limit as usize;
    let agents = if has_more {
        agents[..limit as usize].to_vec()
    } else {
        agents
    };

    Ok(PaginatedResponse {
        data: agents,
        has_more,
        limit: Some(limit as i32),
        offset: query.offset.map(|o| o as i32),
    }
    .into())
}

pub async fn create_ai_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateAgentRequest>,
) -> ApiResult<AiAgent> {
    let configuration = request.configuration.unwrap_or(serde_json::json!({}));

    CreateAiAgentCommand::new(
        deployment_id,
        request.name,
        request.description,
        configuration,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn get_ai_agent_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<AiAgentWithDetails> {
    GetAiAgentByIdQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_ai_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Json(request): Json<UpdateAgentRequest>,
) -> ApiResult<AiAgent> {
    let mut command = UpdateAiAgentCommand::new(deployment_id, params.agent_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(description) = request.description {
        command = command.with_description(Some(description));
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

pub async fn delete_ai_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<()> {
    DeleteAiAgentCommand::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
