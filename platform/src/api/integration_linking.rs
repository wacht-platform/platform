use axum::{Json, extract::State};
use commands::{
    Command, CreateIntegrationLinkCodeCommand, GetActiveIntegrationCommand,
    ValidateLinkCodeCommand,
};
use common::state::AppState;
use serde::{Deserialize, Serialize};

use crate::{application::response::ApiResult, middleware::RequireDeployment};

/// Request to generate a new integration link code
#[derive(Debug, Deserialize)]
pub struct GenerateLinkCodeRequest {
    pub user_id: i64,
    pub agent_id: i64,
    pub integration_type: String,
}

/// Response with the generated link code
#[derive(Debug, Serialize)]
pub struct GenerateLinkCodeResponse {
    pub code: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Generate a new link code for a user to link their external integration
pub async fn generate_link_code(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<GenerateLinkCodeRequest>,
) -> ApiResult<GenerateLinkCodeResponse> {
    let result = CreateIntegrationLinkCodeCommand::new(
        deployment_id,
        request.user_id,
        request.agent_id,
        request.integration_type,
    )
    .execute(&app_state)
    .await?;

    Ok(GenerateLinkCodeResponse {
        code: result.code,
        expires_at: result.expires_at,
    }
    .into())
}

/// Request to validate a link code
#[derive(Debug, Deserialize)]
pub struct ValidateLinkCodeRequest {
    pub code: String,
    pub integration_id: i64,
    pub external_id: String,
    pub external_tenant_id: Option<String>,
    pub connection_metadata: Option<serde_json::Value>,
}

/// Response after validating a link code
#[derive(Debug, Serialize)]
pub struct ValidateLinkCodeResponse {
    pub success: bool,
    pub context_group: String,
    pub connection_id: i64,
}

/// Validate a link code and create the integration connection
pub async fn validate_link_code(
    State(app_state): State<AppState>,
    RequireDeployment(_deployment_id): RequireDeployment,
    Json(request): Json<ValidateLinkCodeRequest>,
) -> ApiResult<ValidateLinkCodeResponse> {
    let result = ValidateLinkCodeCommand::new(
        request.code,
        request.integration_id,
        request.external_id,
        request.external_tenant_id,
        request.connection_metadata.unwrap_or_default(),
    )
    .execute(&app_state)
    .await?;

    Ok(ValidateLinkCodeResponse {
        success: true,
        context_group: result.context_group,
        connection_id: result.connection_id,
    }
    .into())
}

/// Request to get an active integration by external ID
#[derive(Debug, Deserialize)]
pub struct GetActiveIntegrationRequest {
    pub integration_id: i64,
    pub external_id: String,
}

/// Response with the active integration connection
#[derive(Debug, Serialize)]
pub struct GetActiveIntegrationResponse {
    pub found: bool,
    pub context_group: Option<String>,
    pub connection_id: Option<i64>,
}

/// Get an active integration connection by external ID
pub async fn get_active_integration(
    State(app_state): State<AppState>,
    RequireDeployment(_deployment_id): RequireDeployment,
    Json(request): Json<GetActiveIntegrationRequest>,
) -> ApiResult<GetActiveIntegrationResponse> {
    let result = GetActiveIntegrationCommand::new(request.integration_id, request.external_id)
        .execute(&app_state)
        .await?;

    match result {
        Some(connection) => Ok(GetActiveIntegrationResponse {
            found: true,
            context_group: Some(connection.context_group),
            connection_id: Some(connection.id),
        }
        .into()),
        None => Ok(GetActiveIntegrationResponse {
            found: false,
            context_group: None,
            connection_id: None,
        }
        .into()),
    }
}
