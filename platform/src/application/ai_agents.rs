use commands::{
    AgentRoleAgentKind, AttachSubAgentToAgentCommand, CreateAiAgentCommand, DeleteAiAgentCommand,
    DetachSubAgentFromAgentCommand, SetAgentRoleAgentCommand, UpdateAiAgentCommand,
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
    pub tools_count: i64,
    pub knowledge_bases_count: i64,
    pub tools: Vec<serde_json::Value>,
    pub knowledge_bases: Vec<serde_json::Value>,
    pub sub_agents: Option<Vec<i64>>,
    #[serde(default, with = "models::utils::serde::i64_as_string_option")]
    pub reviewer_agent_id: Option<i64>,
    #[serde(default, with = "models::utils::serde::i64_as_string_option")]
    pub conversation_agent_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strong_model: Option<models::AgentModelOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weak_model: Option<models::AgentModelOverride>,
    #[serde(default)]
    pub require_approval_mcp: bool,
    #[serde(default)]
    pub require_approval_virtual: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_approval_rules: Vec<models::AgentToolApprovalRule>,
    #[serde(default)]
    pub hooks: models::AgentHooksConfig,
    pub limits: models::AgentLimits,
    #[serde(default)]
    pub disabled_internal_tools: Vec<String>,
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
    let agent_id = app_state.sf.next_id()? as i64;
    let mut command =
        CreateAiAgentCommand::new(agent_id, deployment_id, request.name, request.description);

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
    if let Some(strong_model) = request.strong_model {
        command = command.with_strong_model(strong_model);
    }
    if let Some(weak_model) = request.weak_model {
        command = command.with_weak_model(weak_model);
    }
    if let Some(hooks) = request.hooks {
        command = command.with_hooks(hooks);
    }
    if let Some(limits) = request.limits {
        command = command.with_limits(limits);
    }
    if let Some(value) = request.require_approval_mcp {
        command = command.with_require_approval_mcp(value);
    }
    if let Some(value) = request.require_approval_virtual {
        command = command.with_require_approval_virtual(value);
    }
    if let Some(rules) = request.tool_approval_rules {
        command = command.with_tool_approval_rules(rules);
    }
    if let Some(tools) = request.disabled_internal_tools {
        command = command.with_disabled_internal_tools(tools);
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
        tools_count: agent.tools_count,
        knowledge_bases_count: agent.knowledge_bases_count,
        tools: vec![],
        knowledge_bases: vec![],
        sub_agents: agent.sub_agents,
        reviewer_agent_id: agent.reviewer_agent_id,
        conversation_agent_id: agent.conversation_agent_id,
        strong_model: agent.strong_model,
        weak_model: agent.weak_model,
        require_approval_mcp: agent.require_approval_mcp,
        require_approval_virtual: agent.require_approval_virtual,
        tool_approval_rules: agent.tool_approval_rules,
        hooks: agent.hooks,
        limits: agent.limits,
        disabled_internal_tools: agent.disabled_internal_tools,
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
    if let Some(tool_ids) = request.tool_ids {
        command = command.with_tool_ids(tool_ids.into_iter().map(i64::from).collect());
    }
    if let Some(knowledge_base_ids) = request.knowledge_base_ids {
        command = command
            .with_knowledge_base_ids(knowledge_base_ids.into_iter().map(i64::from).collect());
    }
    if request.clear_strong_model {
        command = command.clearing_strong_model();
    } else if let Some(strong_model) = request.strong_model {
        command = command.with_strong_model(strong_model);
    }
    if request.clear_weak_model {
        command = command.clearing_weak_model();
    } else if let Some(weak_model) = request.weak_model {
        command = command.with_weak_model(weak_model);
    }
    if let Some(hooks) = request.hooks {
        command = command.with_hooks(hooks);
    }
    if let Some(limits) = request.limits {
        command = command.with_limits(limits);
    }
    if let Some(value) = request.require_approval_mcp {
        command = command.with_require_approval_mcp(value);
    }
    if let Some(value) = request.require_approval_virtual {
        command = command.with_require_approval_virtual(value);
    }
    if let Some(rules) = request.tool_approval_rules {
        command = command.with_tool_approval_rules(rules);
    }
    if let Some(tools) = request.disabled_internal_tools {
        command = command.with_disabled_internal_tools(tools);
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

pub async fn set_agent_role_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    role: &str,
    target_agent_id: Option<i64>,
) -> Result<AiAgentWithDetails, AppError> {
    let role = match role {
        "reviewer" => AgentRoleAgentKind::Reviewer,
        "conversation" => AgentRoleAgentKind::Conversation,
        other => {
            return Err(AppError::BadRequest(format!(
                "unknown role agent kind: {other}"
            )));
        }
    };
    let db_deps = deps::from_app(app_state).db();
    SetAgentRoleAgentCommand::new(deployment_id, agent_id, role, target_agent_id)
        .execute_with_deps(&db_deps)
        .await?;
    get_ai_agent_by_id(app_state, deployment_id, agent_id).await
}
