use crate::{
    application::{HttpState, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{Json, extract::State};
use commands::{
    Command, GenerateAgentContextTokenCommand, GenerateTokenCommand, GenerateTokenResponse,
};
use serde::Deserialize;

#[cfg(feature = "console-api")]
use dto::json::GenerateUserAgentContextTokenRequest;
#[cfg(feature = "console-api")]
use wacht::middleware::extractors::RequireAuth;

#[derive(Debug, Deserialize)]
pub struct GenerateTokenRequest {
    pub session_id: i64,
    pub template: Option<String>,
}

pub async fn generate_token(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<GenerateTokenRequest>,
) -> ApiResult<GenerateTokenResponse> {
    let template_name = request.template.unwrap_or_else(|| "default".to_string());

    GenerateTokenCommand::new(deployment_id, request.session_id, template_name)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

#[derive(Debug, Deserialize)]
pub struct GenerateAgentContextTokenRequest {
    pub user_id: i64,
    pub audience: Option<String>,
    pub validity_hours: Option<u32>, // Optional validity in hours, defaults to 24
}

pub async fn generate_agent_context_token(
    State(app_state): State<HttpState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<GenerateAgentContextTokenRequest>,
) -> ApiResult<GenerateTokenResponse> {
    GenerateAgentContextTokenCommand::new(deployment_id, request.user_id, request.audience)
        .with_validity_hours(request.validity_hours.unwrap_or(24))
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

#[cfg(feature = "console-api")]
pub async fn generate_user_agent_context_token(
    auth: RequireAuth,
    RequireDeployment(_): RequireDeployment,
    Json(request): Json<GenerateUserAgentContextTokenRequest>,
) -> ApiResult<GenerateTokenResponse> {
    let user_id = auth.user_id.parse::<i64>().map_err(|_| {
        crate::application::AppError::BadRequest("Invalid user ID in auth context".to_string())
    })?;

    let agent_token_request = wacht::agents::GenerateAgentContextTokenRequest {
        user_id,
        audience: request.audience,
        validity_hours: request.validity_hours,
    };

    let token_response = wacht::agents::generate_agent_context_token(agent_token_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to generate agent context token via SDK: {:?}", e);
            crate::application::AppError::Internal("Failed to generate token".to_string())
        })?;

    Ok(GenerateTokenResponse {
        token: token_response.token,
        expires: token_response.expires,
    }
    .into())
}
