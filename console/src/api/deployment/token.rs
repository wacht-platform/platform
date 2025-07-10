use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use shared::commands::{Command, GenerateTokenCommand, GenerateTokenResponse};

use crate::{
    application::{HttpState, response::ApiResult},
};

#[derive(Debug, Deserialize)]
pub struct GenerateTokenRequest {
    pub session_id: i64,
    pub template: Option<String>,
}

pub async fn generate_token(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
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