use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use serde::Deserialize;

use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{
    Command, CreateAgentIntegrationCommand, DeleteAgentIntegrationCommand,
    UpdateAgentIntegrationCommand,
};
use models::{AgentIntegration, IntegrationType};
use queries::{GetAgentIntegrationByIdQuery, GetAgentIntegrationsQuery, Query as QueryTrait};

#[derive(Deserialize)]
pub struct IntegrationParams {
    pub integration_id: i64,
}

#[derive(Deserialize)]
pub struct GetIntegrationsQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub integration_type: Option<String>,
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
    match s.to_lowercase().as_str() {
        "teams" => Ok(IntegrationType::Teams),
        "slack" => Ok(IntegrationType::Slack),
        "whatsapp" => Ok(IntegrationType::WhatsApp),
        "discord" => Ok(IntegrationType::Discord),
        _ => Err(format!("Unknown integration type: {}", s)),
    }
}

pub async fn get_agent_integrations(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<GetIntegrationsQuery>,
) -> ApiResult<PaginatedResponse<AgentIntegration>> {
    let limit = query.limit.unwrap_or(50) as u32;

    let integrations = GetAgentIntegrationsQuery::new(deployment_id)
        .with_limit(Some(limit + 1))
        .with_offset(query.offset.map(|o| o as u32))
        .execute(&app_state)
        .await?;

    let has_more = integrations.len() > limit as usize;
    let integrations = if has_more {
        integrations[..limit as usize].to_vec()
    } else {
        integrations
    };

    Ok(PaginatedResponse {
        data: integrations,
        has_more,
        limit: Some(limit as i32),
        offset: query.offset.map(|o| o as i32),
    }
    .into())
}

pub async fn create_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateIntegrationRequest>,
) -> ApiResult<AgentIntegration> {
    let integration_type = parse_integration_type(&request.integration_type)
        .map_err(|e| common::error::AppError::BadRequest(e))?;

    CreateAgentIntegrationCommand::new(deployment_id, integration_type, request.name, request.config)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_agent_integration_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<IntegrationParams>,
) -> ApiResult<AgentIntegration> {
    GetAgentIntegrationByIdQuery::new(deployment_id, params.integration_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<IntegrationParams>,
    Json(request): Json<UpdateIntegrationRequest>,
) -> ApiResult<AgentIntegration> {
    let mut command = UpdateAgentIntegrationCommand::new(deployment_id, params.integration_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(config) = request.config {
        command = command.with_config(config);
    }

    command
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_agent_integration(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<IntegrationParams>,
) -> ApiResult<()> {
    DeleteAgentIntegrationCommand::new(deployment_id, params.integration_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}
