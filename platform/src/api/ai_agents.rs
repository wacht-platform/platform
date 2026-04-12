use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::application::{
    ai_agents as ai_agents_app,
    response::{ApiResult, PaginatedResponse},
};
use common::state::AppState;

use dto::{
    json::deployment::{CreateAgentRequest, UpdateAgentRequest},
    query::deployment::GetAgentsQuery,
};
use models::{AiAgent, AiAgentWithDetails};

pub use ai_agents_app::AgentDetailsResponse;

#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

#[derive(Deserialize)]
pub struct AgentSubAgentParams {
    pub agent_id: i64,
    pub sub_agent_id: i64,
}

pub async fn get_ai_agents(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<GetAgentsQuery>,
) -> ApiResult<PaginatedResponse<AiAgentWithDetails>> {
    let agents = ai_agents_app::get_ai_agents(&app_state, deployment_id, query).await?;
    Ok(agents.into())
}

pub async fn create_ai_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateAgentRequest>,
) -> ApiResult<AiAgent> {
    let agent = ai_agents_app::create_ai_agent(&app_state, deployment_id, request).await?;
    Ok(agent.into())
}

pub async fn get_ai_agent_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<AiAgentWithDetails> {
    let agent =
        ai_agents_app::get_ai_agent_by_id(&app_state, deployment_id, params.agent_id).await?;
    Ok(agent.into())
}

pub async fn get_ai_agent_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<AgentDetailsResponse> {
    let details =
        ai_agents_app::get_ai_agent_details(&app_state, deployment_id, params.agent_id).await?;
    Ok(details.into())
}

pub async fn update_ai_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Json(request): Json<UpdateAgentRequest>,
) -> ApiResult<AiAgent> {
    let agent =
        ai_agents_app::update_ai_agent(&app_state, deployment_id, params.agent_id, request).await?;
    Ok(agent.into())
}

pub async fn delete_ai_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<()> {
    ai_agents_app::delete_ai_agent(&app_state, deployment_id, params.agent_id).await?;
    Ok(().into())
}

pub async fn get_agent_sub_agents(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<PaginatedResponse<AiAgentWithDetails>> {
    let sub_agents =
        ai_agents_app::get_agent_sub_agents(&app_state, deployment_id, params.agent_id).await?;
    Ok(sub_agents.into())
}

pub async fn attach_sub_agent_to_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentSubAgentParams>,
) -> ApiResult<()> {
    ai_agents_app::attach_sub_agent_to_agent(
        &app_state,
        deployment_id,
        params.agent_id,
        params.sub_agent_id,
    )
    .await?;
    Ok(().into())
}

pub async fn detach_sub_agent_from_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentSubAgentParams>,
) -> ApiResult<()> {
    ai_agents_app::detach_sub_agent_from_agent(
        &app_state,
        deployment_id,
        params.agent_id,
        params.sub_agent_id,
    )
    .await?;
    Ok(().into())
}
