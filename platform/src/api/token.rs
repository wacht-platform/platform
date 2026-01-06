use crate::{application::response::ApiResult, middleware::RequireDeployment};
use axum::{Json, extract::State};
use commands::{
    Command, GenerateAgentContextTokenCommand, GenerateTokenCommand, GenerateTokenResponse,
};
use common::state::AppState;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct GenerateTokenRequest {
    pub session_id: i64,
    pub template: Option<String>,
}

pub async fn generate_token(
    State(app_state): State<AppState>,
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
    pub subject: String,
    pub agent_name: String,
    pub validity_hours: Option<u32>,
}

pub async fn generate_agent_context_token(
    State(app_state): State<AppState>,
    RequireDeployment(_deployment_id): RequireDeployment,
    Json(request): Json<GenerateAgentContextTokenRequest>,
) -> ApiResult<GenerateTokenResponse> {
    // TODO: Hardcoded temporarily - should use proper deployment selection
    let deployment_id: i64 = 20220525523509059;
    
    let user_id = request.subject.parse::<i64>().map_err(|_| {
        crate::application::AppError::BadRequest("Invalid subject".to_string())
    })?;
    
    GenerateAgentContextTokenCommand::new(deployment_id, user_id, Some(request.agent_name))
        .with_validity_hours(request.validity_hours.unwrap_or(24))
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}