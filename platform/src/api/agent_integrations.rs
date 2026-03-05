use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::api::pagination::paginate_results;
use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{
    Command, CreateAgentIntegrationCommand, DeleteAgentIntegrationCommand,
    UpdateAgentIntegrationCommand,
};
use models::{AgentIntegration, IntegrationType};
use queries::{GetAgentIntegrationByIdQuery, GetAgentIntegrationsQuery, Query as QueryTrait};
use std::str::FromStr;

const INTEGRATIONS_BETA_DISABLED_MESSAGE: &str =
    "Integrations are a beta feature. Please email us to get access.";

fn integrations_beta_enabled() -> bool {
    true
}

#[derive(Deserialize)]
pub struct AgentIntegrationParams {
    pub agent_id: i64,
    pub integration_id: i64,
}

#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

#[derive(Deserialize)]
pub struct GetIntegrationsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateIntegrationRequest {
    pub integration_type: String,
    pub name: String,
    pub config: serde_json::Value,
}

#[derive(Deserialize)]
pub struct UpdateIntegrationRequest {
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
}

fn parse_integration_type(s: &str) -> Result<IntegrationType, String> {
    let parsed = IntegrationType::from_str(s)?;
    match parsed {
        IntegrationType::Teams | IntegrationType::ClickUp => Ok(parsed),
        _ => Err("Only 'teams' and 'clickup' integrations are supported".to_string()),
    }
}

fn is_console_supported_integration_type(integration_type: IntegrationType) -> bool {
    matches!(
        integration_type,
        IntegrationType::Teams | IntegrationType::ClickUp
    )
}

fn normalize_integration_config(
    integration_type: IntegrationType,
    config: serde_json::Value,
) -> Result<serde_json::Value, common::error::AppError> {
    match integration_type {
        IntegrationType::Mcp => Err(common::error::AppError::BadRequest(
            "MCP servers must be managed via /ai/mcp-servers APIs".to_string(),
        )),
        IntegrationType::Teams | IntegrationType::ClickUp => Ok(config),
    }
}

/// GET /agents/:agent_id/integrations
pub async fn get_agent_integrations(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Query(query): Query<GetIntegrationsQuery>,
) -> ApiResult<PaginatedResponse<AgentIntegration>> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset;

    let integrations = GetAgentIntegrationsQuery::new(deployment_id, params.agent_id)
        .with_limit(Some(limit as u32 + 1))
        .with_offset(offset.map(|o| o as u32))
        .execute(&app_state)
        .await?;

    let integrations: Vec<AgentIntegration> = integrations
        .into_iter()
        .filter(|integration| is_console_supported_integration_type(integration.integration_type))
        .collect();

    Ok(paginate_results(integrations, limit as i32, offset).into())
}

/// POST /agents/:agent_id/integrations
pub async fn create_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
    Json(request): Json<CreateIntegrationRequest>,
) -> ApiResult<AgentIntegration> {
    if !integrations_beta_enabled() {
        return Err(common::error::AppError::Forbidden(
            INTEGRATIONS_BETA_DISABLED_MESSAGE.to_string(),
        )
        .into());
    }

    let integration_type = parse_integration_type(&request.integration_type)
        .map_err(|e| common::error::AppError::BadRequest(e))?;
    let normalized_config = normalize_integration_config(integration_type, request.config)?;

    CreateAgentIntegrationCommand::new(
        deployment_id,
        params.agent_id,
        integration_type,
        request.name,
        normalized_config,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

/// GET /agents/:agent_id/integrations/:integration_id
pub async fn get_agent_integration_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentIntegrationParams>,
) -> ApiResult<AgentIntegration> {
    GetAgentIntegrationByIdQuery::new(deployment_id, params.agent_id, params.integration_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

/// PATCH /agents/:agent_id/integrations/:integration_id
pub async fn update_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentIntegrationParams>,
    Json(request): Json<UpdateIntegrationRequest>,
) -> ApiResult<AgentIntegration> {
    let mut command = UpdateAgentIntegrationCommand::new(deployment_id, params.integration_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(config) = request.config {
        let existing_integration = GetAgentIntegrationByIdQuery::new(
            deployment_id,
            params.agent_id,
            params.integration_id,
        )
        .execute(&app_state)
        .await?;
        if !is_console_supported_integration_type(existing_integration.integration_type) {
            return Err(common::error::AppError::BadRequest(
                "Only 'teams' and 'clickup' integrations are supported".to_string(),
            )
            .into());
        }
        let normalized_config =
            normalize_integration_config(existing_integration.integration_type, config)?;
        command = command.with_config(normalized_config);
    }

    command
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

/// DELETE /agents/:agent_id/integrations/:integration_id
pub async fn delete_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentIntegrationParams>,
) -> ApiResult<()> {
    DeleteAgentIntegrationCommand::new(deployment_id, params.integration_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
