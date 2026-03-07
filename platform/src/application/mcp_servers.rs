use commands::{
    AttachMcpServerToAgentCommand, CreateMcpServerCommand, DeleteMcpServerCommand,
    DetachMcpServerFromAgentCommand, UpdateMcpServerCommand,
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use models::{McpServer, McpServerConfig};
use queries::{GetAgentMcpServersQuery, GetMcpServerByIdQuery, GetMcpServersQuery};

use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};

pub async fn get_mcp_servers(
    app_state: &AppState,
    deployment_id: i64,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<PaginatedResponse<McpServer>, common::error::AppError> {
    let limit = limit.unwrap_or(50);
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let servers = GetMcpServersQuery::new(deployment_id)
        .with_limit(Some(limit as u32 + 1))
        .with_offset(offset.map(|o| o as u32))
        .execute_with_db(reader)
        .await?;

    Ok(paginate_results(servers, limit as i32, offset))
}

pub async fn create_mcp_server(
    app_state: &AppState,
    deployment_id: i64,
    name: String,
    config: McpServerConfig,
) -> Result<McpServer, common::error::AppError> {
    CreateMcpServerCommand::new(app_state.sf.next_id()? as i64, deployment_id, name, config)
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn get_mcp_server_by_id(
    app_state: &AppState,
    deployment_id: i64,
    mcp_server_id: i64,
) -> Result<McpServer, common::error::AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetMcpServerByIdQuery::new(deployment_id, mcp_server_id)
        .execute_with_db(reader)
        .await
}

pub async fn update_mcp_server(
    app_state: &AppState,
    deployment_id: i64,
    mcp_server_id: i64,
    name: Option<String>,
    config: Option<McpServerConfig>,
) -> Result<McpServer, common::error::AppError> {
    let mut command = UpdateMcpServerCommand::new(deployment_id, mcp_server_id);

    if let Some(name) = name {
        command = command.with_name(name);
    }
    if let Some(config) = config {
        command = command.with_config(config);
    }

    command.execute_with_db(app_state.db_router.writer()).await
}

pub async fn delete_mcp_server(
    app_state: &AppState,
    deployment_id: i64,
    mcp_server_id: i64,
) -> Result<(), common::error::AppError> {
    DeleteMcpServerCommand::new(deployment_id, mcp_server_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn get_agent_mcp_servers(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<PaginatedResponse<McpServer>, common::error::AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let servers = GetAgentMcpServersQuery::new(deployment_id, agent_id)
        .execute_with_db(reader)
        .await?;
    Ok(PaginatedResponse::from(servers))
}

pub async fn attach_mcp_server_to_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    mcp_server_id: i64,
) -> Result<(), common::error::AppError> {
    AttachMcpServerToAgentCommand::new(deployment_id, agent_id, mcp_server_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn detach_mcp_server_from_agent(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    mcp_server_id: i64,
) -> Result<(), common::error::AppError> {
    DetachMcpServerFromAgentCommand::new(deployment_id, agent_id, mcp_server_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}
