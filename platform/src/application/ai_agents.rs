use commands::{
    AttachSubAgentToAgentCommand, CreateAiAgentCommand, DeleteAiAgentCommand,
    DetachSubAgentFromAgentCommand, UpdateAiAgentCommand,
};
use common::ReadConsistency;
use common::error::AppError;
use dto::{
    json::deployment::{CreateAgentRequest, UpdateAgentRequest},
    query::deployment::GetAgentsQuery,
};
use models::{AiAgent, AiAgentWithDetails, IntegrationType};
use queries::{
    GetAgentIntegrationsQuery, GetAiAgentByIdQuery, GetAiAgentsByIdsQuery, GetAiAgentsQuery,
};

use crate::{
    api::pagination::paginate_results,
    application::{AppState, response::PaginatedResponse},
};
use crate::application::deps;

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
    pub sub_agents: Option<Vec<i64>>,
    pub spawn_config: Option<models::SpawnConfig>,
}

pub async fn get_ai_agents(
    app_state: &AppState,
    deployment_id: i64,
    query: GetAgentsQuery,
) -> Result<PaginatedResponse<AiAgentWithDetails>, AppError> {
    let limit = query.limit.unwrap_or(50) as i32;
    let query_limit = limit as u32;
    let offset = query.offset.map(|o| o as i64);

    let agents = GetAiAgentsQuery::new(deployment_id)
        .with_limit(Some(query_limit + 1))
        .with_offset(offset.map(|o| o as u32))
        .with_search(query.search)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(paginate_results(agents, limit, offset))
}

pub async fn create_ai_agent(
    app_state: &AppState,
    deployment_id: i64,
    request: CreateAgentRequest,
) -> Result<AiAgent, AppError> {
    let configuration = request.configuration.unwrap_or(serde_json::json!({}));
    let agent_id = app_state.sf.next_id()? as i64;
    let mut command = CreateAiAgentCommand::new(
        agent_id,
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
        .execute_with_deps(&deps::from_app(app_state).db())
        .await
}

pub async fn get_ai_agent_by_id(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<AiAgentWithDetails, AppError> {
    GetAiAgentByIdQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn get_ai_agent_details(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<AgentDetailsResponse, AppError> {
    let agent = GetAiAgentByIdQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let integrations = GetAgentIntegrationsQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
        .unwrap_or_default();

    let base_url = "https://agentlink.wacht.services".to_string();
    let integrations_with_urls: Vec<IntegrationWithUrl> = integrations
        .into_iter()
        .filter(|integration| {
            matches!(
                integration.integration_type,
                IntegrationType::Teams | IntegrationType::ClickUp
            )
        })
        .map(|integration| {
            let webhook_url = match integration.integration_type {
                IntegrationType::Teams => Some(format!(
                    "{}/service/{}/{}/message",
                    base_url,
                    integration.integration_type.to_string().to_lowercase(),
                    agent_id
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
        tools: vec![],
        knowledge_bases: vec![],
        sub_agents: agent.sub_agents,
        spawn_config: agent.spawn_config,
    })
}

pub async fn update_ai_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    request: UpdateAgentRequest,
) -> Result<AiAgent, AppError> {
    let mut command = UpdateAiAgentCommand::new(deployment_id, agent_id);

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
        .execute_with_deps(&deps::from_app(app_state).db())
        .await
}

pub async fn delete_ai_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<(), AppError> {
    DeleteAiAgentCommand::new(deployment_id, agent_id)
        .execute_with_deps(&deps::from_app(app_state).db())
        .await?;
    Ok(())
}

pub async fn get_agent_sub_agents(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<PaginatedResponse<AiAgentWithDetails>, AppError> {
    let parent_agent = GetAiAgentByIdQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let sub_agent_ids = parent_agent.sub_agents.unwrap_or_default();
    let sub_agents = GetAiAgentsByIdsQuery::new(deployment_id, sub_agent_ids)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(PaginatedResponse::from(sub_agents))
}

pub async fn attach_sub_agent_to_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    sub_agent_id: i64,
) -> Result<(), AppError> {
    AttachSubAgentToAgentCommand::new(deployment_id, agent_id, sub_agent_id)
        .execute_with_deps(&deps::from_app(app_state).db())
        .await?;
    Ok(())
}

pub async fn detach_sub_agent_from_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    sub_agent_id: i64,
) -> Result<(), AppError> {
    DetachSubAgentFromAgentCommand::new(deployment_id, agent_id, sub_agent_id)
        .execute_with_deps(&deps::from_app(app_state).db())
        .await?;
    Ok(())
}

pub async fn get_agent_integrations(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<IntegrationsResponse, AppError> {
    let integrations = GetAgentIntegrationsQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let base_url = "https://agentlink.wacht.services".to_string();
    let integrations_with_urls: Vec<IntegrationWithUrl> = integrations
        .into_iter()
        .filter(|integration| {
            matches!(
                integration.integration_type,
                IntegrationType::Teams | IntegrationType::ClickUp
            )
        })
        .map(|integration| {
            let webhook_url = match integration.integration_type {
                IntegrationType::Teams => Some(format!(
                    "{}/service/{}/{}/message",
                    base_url,
                    integration.integration_type.to_string().to_lowercase(),
                    agent_id
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
    })
}
