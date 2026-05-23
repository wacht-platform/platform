use crate::{
    application::ai_settings as ai_settings_app, application::response::ApiResult,
    middleware::RequireDeployment,
};
use common::state::AppState;

use models::{
    CreateDeploymentAiProviderProfileRequest, DeploymentAiProviderProfileResponse,
    DeploymentAiSettingsResponse, UpdateDeploymentAiProviderProfileRequest,
    UpdateDeploymentAiSettingsRequest,
};
use serde::Deserialize;

use axum::{
    Json,
    extract::{Path, State},
};

#[derive(Deserialize)]
pub struct AiProviderProfileParams {
    pub profile_id: i64,
}

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

pub async fn list_ai_provider_profiles(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<Vec<DeploymentAiProviderProfileResponse>> {
    let profiles = ai_settings_app::list_ai_provider_profiles(&app_state, deployment_id).await?;
    Ok(profiles.into())
}

pub async fn create_ai_provider_profile(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateDeploymentAiProviderProfileRequest>,
) -> ApiResult<DeploymentAiProviderProfileResponse> {
    let profile =
        ai_settings_app::create_ai_provider_profile(&app_state, deployment_id, request).await?;
    Ok(profile.into())
}

pub async fn get_ai_provider_profile(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AiProviderProfileParams>,
) -> ApiResult<DeploymentAiProviderProfileResponse> {
    let profile =
        ai_settings_app::get_ai_provider_profile(&app_state, deployment_id, params.profile_id)
            .await?;
    Ok(profile.into())
}

pub async fn update_ai_provider_profile(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AiProviderProfileParams>,
    Json(request): Json<UpdateDeploymentAiProviderProfileRequest>,
) -> ApiResult<DeploymentAiProviderProfileResponse> {
    let profile = ai_settings_app::update_ai_provider_profile(
        &app_state,
        deployment_id,
        params.profile_id,
        request,
    )
    .await?;
    Ok(profile.into())
}

pub async fn delete_ai_provider_profile(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AiProviderProfileParams>,
) -> ApiResult<()> {
    ai_settings_app::delete_ai_provider_profile(&app_state, deployment_id, params.profile_id)
        .await?;
    Ok(().into())
}
