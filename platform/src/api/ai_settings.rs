use crate::{
    application::ai_settings as ai_settings_app, application::response::ApiResult,
    middleware::RequireDeployment,
};
use common::state::AppState;

use models::{DeploymentAiSettingsResponse, UpdateDeploymentAiSettingsRequest};

use axum::{Json, extract::State};

/// GET /settings/ai-settings - Fetch AI settings (keys masked)
pub async fn get_ai_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<DeploymentAiSettingsResponse> {
    let response = ai_settings_app::get_ai_settings(&app_state, deployment_id).await?;
    Ok(response.into())
}

/// PUT /settings/ai-settings - Update AI settings
pub async fn update_ai_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<UpdateDeploymentAiSettingsRequest>,
) -> ApiResult<DeploymentAiSettingsResponse> {
    let settings = ai_settings_app::update_ai_settings(&app_state, deployment_id, updates).await?;
    Ok(settings.into())
}
