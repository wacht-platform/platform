use common::error::AppError;
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<DeploymentAiSettings>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            SELECT
                id,
                deployment_id,
                strong_llm_provider,
                weak_llm_provider,
                gemini_api_key,
                openrouter_api_key,
                openrouter_require_parameters,
                openai_api_key,
                anthropic_api_key,
                strong_model,
                weak_model,
                storage_provider,
                storage_bucket,
                storage_region,
                storage_endpoint,
                storage_root_prefix,
                storage_force_path_style,
                storage_access_key_id,
                storage_secret_access_key,
                vector_store_initialized_at,
                created_at,
                updated_at
            FROM deployment_ai_settings
            WHERE deployment_id = $1
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(executor)
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
