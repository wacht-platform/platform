use commands::UpdateDeploymentAiSettingsCommand;
use common::db_router::ReadConsistency;
use common::error::AppError;
use models::plan_features::PlanFeature;
use models::{DeploymentAiSettingsResponse, UpdateDeploymentAiSettingsRequest};
use queries::{GetDeploymentAiSettingsQuery, plan_access::CheckDeploymentFeatureAccessQuery};

use crate::application::AppState;
use common::deps;

pub async fn get_ai_settings(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentAiSettingsResponse, AppError> {
    let settings = GetDeploymentAiSettingsQuery::builder()
        .deployment_id(deployment_id)
        .build()?
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(match settings {
        Some(settings) => DeploymentAiSettingsResponse::from(settings),
        None => DeploymentAiSettingsResponse {
            gemini_api_key_set: false,
            openai_api_key_set: false,
            anthropic_api_key_set: false,
        },
    })
}

pub async fn update_ai_settings(
    app_state: &AppState,
    deployment_id: i64,
    updates: UpdateDeploymentAiSettingsRequest,
) -> Result<DeploymentAiSettingsResponse, AppError> {
    let has_ai_access = CheckDeploymentFeatureAccessQuery::builder()
        .deployment_id(deployment_id)
        .feature(PlanFeature::AiAgents)
        .build()?
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await
        .map_err(|e| AppError::Internal(format!("Failed to check AI feature access: {}", e)))?;

    if !has_ai_access {
        return Err(AppError::Forbidden(
            "AI agent usage requires Growth plan".to_string(),
        ));
    }

    let deps = deps::from_app(app_state).db().enc();
    let settings = UpdateDeploymentAiSettingsCommand::builder()
        .deployment_id(deployment_id)
        .updates(updates)
        .build()?
        .execute_with_deps(&deps)
        .await?;

    Ok(DeploymentAiSettingsResponse::from(settings))
}
