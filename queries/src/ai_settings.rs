use crate::Query;
use common::error::AppError;
use common::state::AppState;
use models::DeploymentAiSettings;

/// Query to fetch AI settings for a deployment
pub struct GetDeploymentAiSettingsQuery {
    deployment_id: i64,
}

#[derive(Default)]
pub struct GetDeploymentAiSettingsQueryBuilder {
    deployment_id: Option<i64>,
}

impl GetDeploymentAiSettingsQuery {
    pub fn builder() -> GetDeploymentAiSettingsQueryBuilder {
        GetDeploymentAiSettingsQueryBuilder::default()
    }

    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with(
        &self,
        pool: &sqlx::PgPool,
    ) -> Result<Option<DeploymentAiSettings>, AppError> {
        let result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            SELECT id, deployment_id, gemini_api_key, openai_api_key, anthropic_api_key, created_at, updated_at
            FROM deployment_ai_settings
            WHERE deployment_id = $1
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(pool)
        .await?;

        Ok(result)
    }
}

impl GetDeploymentAiSettingsQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn build(self) -> Result<GetDeploymentAiSettingsQuery, AppError> {
        Ok(GetDeploymentAiSettingsQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
        })
    }
}

impl Query for GetDeploymentAiSettingsQuery {
    type Output = Option<DeploymentAiSettings>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}
