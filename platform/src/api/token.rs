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
    pub context_subject: Option<String>,
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
        request.context_subject,
    )
    .with_validity_hours(request.validity_hours.unwrap_or(24))
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}