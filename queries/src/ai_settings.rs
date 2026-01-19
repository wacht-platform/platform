use crate::Query;
use common::error::AppError;
use common::state::AppState;
use models::DeploymentAiSettings;

/// Query to fetch AI settings for a deployment
pub struct GetDeploymentAiSettingsQuery {
    deployment_id: i64,
}

impl GetDeploymentAiSettingsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentAiSettingsQuery {
    type Output = Option<DeploymentAiSettings>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            SELECT id, deployment_id, gemini_api_key, openai_api_key, anthropic_api_key, created_at, updated_at
            FROM deployment_ai_settings
            WHERE deployment_id = $1
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(result)
    }
}
