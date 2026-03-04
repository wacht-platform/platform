use crate::{application::response::ApiResult, middleware::RequireDeployment};
use common::state::AppState;

use commands::{Command, UpdateDeploymentAiSettingsCommand};
use common::error::AppError;
use models::plan_features::PlanFeature;
use models::{DeploymentAiSettingsResponse, UpdateDeploymentAiSettingsRequest};
use queries::{
    GetDeploymentAiSettingsQuery, Query, plan_access::CheckDeploymentFeatureAccessQuery,
};

use axum::{Json, extract::State};

/// GET /settings/ai-settings - Fetch AI settings (keys masked)
pub async fn get_ai_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<DeploymentAiSettingsResponse> {
    let settings = GetDeploymentAiSettingsQuery::new(deployment_id)
        .execute(&app_state)
        .await?;

    let response = match settings {
        Some(s) => DeploymentAiSettingsResponse::from(s),
        None => DeploymentAiSettingsResponse {
            gemini_api_key_set: false,
            openai_api_key_set: false,
            anthropic_api_key_set: false,
        },
    };

    Ok(response.into())
}

/// PUT /settings/ai-settings - Update AI settings
pub async fn update_ai_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(updates): Json<UpdateDeploymentAiSettingsRequest>,
) -> ApiResult<DeploymentAiSettingsResponse> {
    let has_ai_access =
        CheckDeploymentFeatureAccessQuery::new(deployment_id, PlanFeature::AiAgents)
            .execute(&app_state)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to check AI feature access: {}", e)))?;

    if !has_ai_access {
        return Err(AppError::Forbidden("AI agent usage requires Growth plan".to_string()).into());
    }

    let settings = UpdateDeploymentAiSettingsCommand::new(deployment_id, updates)
        .execute(&app_state)
        .await?;

    Ok(DeploymentAiSettingsResponse::from(settings).into())
}
