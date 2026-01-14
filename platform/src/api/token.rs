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