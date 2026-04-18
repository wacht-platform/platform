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
use models::{AiAgent, AiAgentWithDetails};
use queries::{GetAiAgentByIdQuery, GetAiAgentsByIdsQuery, GetAiAgentsQuery};

use crate::{
    api::pagination::paginate_results,
    application::{AppState, response::PaginatedResponse},
};
use common::deps;

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
    pub tools: Vec<serde_json::Value>,
    pub knowledge_bases: Vec<serde_json::Value>,
    pub sub_agents: Option<Vec<i64>>,
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
    let db_deps = deps::from_app(app_state).db();
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
        command = command.with_sub_agents(sub_agents.into_iter().map(i64::from).collect());
    }
    if let Some(tool_ids) = request.tool_ids {
        command = command.with_tool_ids(tool_ids.into_iter().map(i64::from).collect());
    }
    if let Some(knowledge_base_ids) = request.knowledge_base_ids {
        command = command
            .with_knowledge_base_ids(knowledge_base_ids.into_iter().map(i64::from).collect());
    }
    command.execute_with_deps(&db_deps).await
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
        tools: vec![],
        knowledge_bases: vec![],
        sub_agents: agent.sub_agents,
    })
}

pub async fn update_ai_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    request: UpdateAgentRequest,
) -> Result<AiAgent, AppError> {
    let db_deps = deps::from_app(app_state).db();
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
        command = command.with_tool_ids(tool_ids.into_iter().map(i64::from).collect());
    }
    if let Some(knowledge_base_ids) = request.knowledge_base_ids {
        command = command
            .with_knowledge_base_ids(knowledge_base_ids.into_iter().map(i64::from).collect());
    }
    if let Some(sub_agents) = request.sub_agents {
        command = command.with_sub_agents(sub_agents.into_iter().map(i64::from).collect());
    }
    command.execute_with_deps(&db_deps).await
}

pub async fn delete_ai_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<(), AppError> {
    let db_deps = deps::from_app(app_state).db();
    DeleteAiAgentCommand::new(deployment_id, agent_id)
        .execute_with_deps(&db_deps)
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
    let db_deps = deps::from_app(app_state).db();
    AttachSubAgentToAgentCommand::new(deployment_id, agent_id, sub_agent_id)
        .execute_with_deps(&db_deps)
        .await?;
    Ok(())
}

pub async fn detach_sub_agent_from_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    sub_agent_id: i64,
) -> Result<(), AppError> {
    let db_deps = deps::from_app(app_state).db();
    DetachSubAgentFromAgentCommand::new(deployment_id, agent_id, sub_agent_id)
        .execute_with_deps(&db_deps)
        .await?;
    Ok(())
}
