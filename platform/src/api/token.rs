use crate::{
    application::{HttpState, response::ApiResult},
    middleware::RequireDeployment,
};
use axum::{
    extract::State,
    Json,
};
use serde::Deserialize;
use commands::{Command, GenerateTokenCommand, GenerateAgentContextTokenCommand, GenerateTokenResponse};

#[cfg(feature = "console-api")]
use crate::middleware::ConsoleDeployment;
#[cfg(feature = "console-api")]
use dto::json::GenerateUserAgentContextTokenRequest;

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
    
    GenerateTokenCommand::new(
        deployment_id,
        request.session_id,
        template_name,
    )
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
    GenerateAgentContextTokenCommand::new(
        deployment_id,
        request.user_id,
        request.audience,
    )
    .with_validity_hours(request.validity_hours.unwrap_or(24))
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}


#[cfg(feature = "console-api")]
pub async fn generate_user_agent_context_token(
    State(_app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<GenerateUserAgentContextTokenRequest>,
) -> ApiResult<GenerateTokenResponse> {
    // For console API, use console deployment ID as user ID
    let user_id = console_deployment_id;
    
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
    }.into())
}