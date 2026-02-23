use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{
    AttachSubAgentToAgentCommand, Command, CreateAiAgentCommand, DeleteAiAgentCommand,
    DetachSubAgentFromAgentCommand, UpdateAiAgentCommand,
};
use dto::{
    json::deployment::{CreateAgentRequest, UpdateAgentRequest},
    query::deployment::GetAgentsQuery,
};
use models::{AiAgent, AiAgentWithDetails};
use queries::{GetAiAgentByIdQuery, GetAiAgentsByIdsQuery, GetAiAgentsQuery, Query as QueryTrait};

// Unified parameter extraction for AI agent routes
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

    let mut command = CreateAiAgentCommand::new(
        deployment_id,
        request.name,
        request.description,
        configuration,
    );

    if let Some(sub_agents) = request.sub_agents {
        command = command.with_sub_agents(sub_agents);
    }
    if let Some(tool_ids) = request.tool_ids {
        command = command.with_tool_ids(tool_ids);
    }
    if let Some(knowledge_base_ids) = request.knowledge_base_ids {
        command = command.with_knowledge_base_ids(knowledge_base_ids);
    }
    if let Some(spawn_config) = request.spawn_config {
        command = command.with_spawn_config(spawn_config);
    }

    command
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

/// Get full agent details including integrations, tools, knowledge bases
pub async fn get_ai_agent_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<AgentDetailsResponse> {
    use queries::GetAgentIntegrationsQuery;

    // Fetch agent
    let agent = GetAiAgentByIdQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await?;

    // Fetch integrations
    let integrations = GetAgentIntegrationsQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await
        .unwrap_or_default();

    let base_url = "https://agentlink.wacht.services".to_string();

    let integrations_with_urls: Vec<IntegrationWithUrl> = integrations
        .into_iter()
        .filter(|integration| {
            matches!(
                integration.integration_type,
                models::IntegrationType::Teams | models::IntegrationType::ClickUp
            )
        })
        .map(|integration| {
            let webhook_url = match integration.integration_type {
                models::IntegrationType::Teams => Some(format!(
                    "{}/service/{}/{}/message",
                    base_url,
                    integration.integration_type.to_string().to_lowercase(),
                    params.agent_id
                )),
                _ => None,
            };
            IntegrationWithUrl {
                id: integration.id,
                created_at: integration.created_at,
                updated_at: integration.updated_at,
                deployment_id: integration.deployment_id,
                agent_id: integration.agent_id,
                integration_type: integration.integration_type,
                name: integration.name,
                config: integration.config,
                webhook_url,
            }
        })
        .collect();

    Ok(AgentDetailsResponse {
        id: agent.id,
        created_at: agent.created_at,
        updated_at: agent.updated_at,
        deployment_id: agent.deployment_id,
        name: agent.name,
        description: agent.description,
        configuration: agent.configuration,
        tools_count: agent.tools_count,
        knowledge_bases_count: agent.knowledge_bases_count,
        integrations: integrations_with_urls,
        tools: vec![],           // TODO: fetch tools if needed
        knowledge_bases: vec![], // TODO: fetch knowledge bases if needed
        sub_agents: agent.sub_agents,
        spawn_config: agent.spawn_config,
    }
    .into())
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
    if let Some(tool_ids) = request.tool_ids {
        command = command.with_tool_ids(tool_ids);
    }
    if let Some(knowledge_base_ids) = request.knowledge_base_ids {
        command = command.with_knowledge_base_ids(knowledge_base_ids);
    }
    if let Some(sub_agents) = request.sub_agents {
        command = command.with_sub_agents(sub_agents);
    }
    if let Some(spawn_config) = request.spawn_config {
        command = command.with_spawn_config(spawn_config);
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

/// Get sub-agents attached to an agent
pub async fn get_agent_sub_agents(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<PaginatedResponse<AiAgentWithDetails>> {
    let parent_agent = GetAiAgentByIdQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await?;

    let sub_agent_ids = parent_agent.sub_agents.unwrap_or_default();
    let sub_agents = GetAiAgentsByIdsQuery::new(deployment_id, sub_agent_ids)
        .execute(&app_state)
        .await?;

    Ok(PaginatedResponse::from(sub_agents).into())
}

/// Attach a sub-agent to an agent
pub async fn attach_sub_agent_to_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentSubAgentParams>,
) -> ApiResult<()> {
    AttachSubAgentToAgentCommand::new(deployment_id, params.agent_id, params.sub_agent_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

/// Detach a sub-agent from an agent
pub async fn detach_sub_agent_from_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentSubAgentParams>,
) -> ApiResult<()> {
    DetachSubAgentFromAgentCommand::new(deployment_id, params.agent_id, params.sub_agent_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

/// Get integrations attached to an agent
pub async fn get_agent_integrations(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<IntegrationsResponse> {
    use queries::GetAgentIntegrationsQuery;

    let integrations = GetAgentIntegrationsQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await?;

    let base_url = "https://agentlink.wacht.services".to_string();

    let integrations_with_urls: Vec<IntegrationWithUrl> = integrations
        .into_iter()
        .filter(|integration| {
            matches!(
                integration.integration_type,
                models::IntegrationType::Teams | models::IntegrationType::ClickUp
            )
        })
        .map(|integration| {
            let webhook_url = match integration.integration_type {
                models::IntegrationType::Teams => Some(format!(
                    "{}/service/{}/{}/message",
                    base_url,
                    integration.integration_type.to_string().to_lowercase(),
                    params.agent_id
                )),
                _ => None,
            };
            IntegrationWithUrl {
                id: integration.id,
                created_at: integration.created_at,
                updated_at: integration.updated_at,
                deployment_id: integration.deployment_id,
                agent_id: integration.agent_id,
                integration_type: integration.integration_type,
                name: integration.name,
                config: integration.config,
                webhook_url,
            }
        })
        .collect();

    Ok(IntegrationsResponse {
        integrations: integrations_with_urls,
    }
    .into())
}

#[derive(serde::Serialize)]
pub struct IntegrationWithUrl {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub agent_id: i64,
    pub integration_type: models::IntegrationType,
    pub name: String,
    pub config: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

#[derive(serde::Serialize)]
pub struct IntegrationsResponse {
    pub integrations: Vec<IntegrationWithUrl>,
}

#[derive(serde::Serialize)]
pub struct AgentDetailsResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub configuration: serde_json::Value,
    pub tools_count: i64,
    pub knowledge_bases_count: i64,
    pub integrations: Vec<IntegrationWithUrl>,
    pub tools: Vec<serde_json::Value>,
    pub knowledge_bases: Vec<serde_json::Value>,
    /// Agents this agent can spawn as sub-agents
    pub sub_agents: Option<Vec<i64>>,
    /// Spawn configuration
    pub spawn_config: Option<models::SpawnConfig>,
}
