use commands::{
    CreateAgentIntegrationCommand, DeleteAgentIntegrationCommand, UpdateAgentIntegrationCommand,
};
use common::db_router::ReadConsistency;
use common::error::AppError;
use models::{AgentIntegration, IntegrationType};
use queries::{GetAgentIntegrationByIdQuery, GetAgentIntegrationsQuery};
use std::str::FromStr;

use crate::application::AppState;

const INTEGRATIONS_BETA_DISABLED_MESSAGE: &str =
    "Integrations are a beta feature. Please email us to get access.";

fn integrations_beta_enabled() -> bool {
    true
}

pub fn ensure_integrations_beta_enabled() -> Result<(), AppError> {
    if integrations_beta_enabled() {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            INTEGRATIONS_BETA_DISABLED_MESSAGE.to_string(),
        ))
    }
}

pub fn parse_integration_type(s: &str) -> Result<IntegrationType, AppError> {
    let parsed = IntegrationType::from_str(s).map_err(AppError::BadRequest)?;
    match parsed {
        IntegrationType::Teams | IntegrationType::ClickUp => Ok(parsed),
        _ => Err(AppError::BadRequest(
            "Only 'teams' and 'clickup' integrations are supported".to_string(),
        )),
    }
}

pub fn is_console_supported_integration_type(integration_type: IntegrationType) -> bool {
    matches!(
        integration_type,
        IntegrationType::Teams | IntegrationType::ClickUp
    )
}

pub fn normalize_integration_config(
    integration_type: IntegrationType,
    config: serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    match integration_type {
        IntegrationType::Mcp => Err(AppError::BadRequest(
            "MCP servers must be managed via /ai/mcp-servers APIs".to_string(),
        )),
        IntegrationType::Teams | IntegrationType::ClickUp => Ok(config),
    }
}

pub async fn list_agent_integrations(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    limit: i64,
    offset: Option<i64>,
) -> Result<Vec<AgentIntegration>, AppError> {
    let integrations = GetAgentIntegrationsQuery::builder()
        .deployment_id(deployment_id)
        .agent_id(agent_id)
        .limit(Some(limit as u32 + 1))
        .offset(offset.map(|o| o as u32))
        .build()?
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(integrations
        .into_iter()
        .filter(|integration| is_console_supported_integration_type(integration.integration_type))
        .collect())
}

pub async fn create_agent_integration(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    integration_type: IntegrationType,
    name: String,
    config: serde_json::Value,
) -> Result<AgentIntegration, AppError> {
    let integration_id = app_state.sf.next_id()? as i64;
    CreateAgentIntegrationCommand::builder()
        .id(integration_id)
        .deployment_id(deployment_id)
        .agent_id(agent_id)
        .integration_type(integration_type)
        .name(name)
        .config(config)
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn get_agent_integration_by_id(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    integration_id: i64,
) -> Result<AgentIntegration, AppError> {
    GetAgentIntegrationByIdQuery::builder()
        .deployment_id(deployment_id)
        .agent_id(agent_id)
        .integration_id(integration_id)
        .build()?
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
}

pub async fn update_agent_integration(
    app_state: &AppState,
    deployment_id: i64,
    integration_id: i64,
    name: Option<String>,
    config: Option<serde_json::Value>,
    existing_integration_type: Option<IntegrationType>,
) -> Result<AgentIntegration, AppError> {
    let mut builder = UpdateAgentIntegrationCommand::builder()
        .deployment_id(deployment_id)
        .integration_id(integration_id)
        .name(name);

    if let Some(config) = config {
        let integration_type = existing_integration_type.ok_or_else(|| {
            AppError::BadRequest("Missing existing integration type for config update".to_string())
        })?;
        let normalized_config = normalize_integration_config(integration_type, config)?;
        builder = builder.config(Some(normalized_config));
    }

    builder
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn delete_agent_integration(
    app_state: &AppState,
    deployment_id: i64,
    integration_id: i64,
) -> Result<(), AppError> {
    DeleteAgentIntegrationCommand::builder()
        .deployment_id(deployment_id)
        .integration_id(integration_id)
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}
