use common::error::AppError;
use models::DeploymentAiProviderProfile;

pub struct ListDeploymentAiProviderProfilesQuery {
    deployment_id: i64,
}

impl ListDeploymentAiProviderProfilesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<DeploymentAiProviderProfile>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query_as!(
            DeploymentAiProviderProfile,
            r#"
            SELECT
                id, deployment_id, provider, name, slug, api_key, base_url,
                organization, project, default_model, enabled, created_at, updated_at
            FROM deployment_ai_provider_profiles
            WHERE deployment_id = $1
            ORDER BY created_at DESC
            "#,
            self.deployment_id
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)
    }
}

pub struct GetDeploymentAiProviderProfileQuery {
    deployment_id: i64,
    profile_id: i64,
}

impl GetDeploymentAiProviderProfileQuery {
    pub fn new(deployment_id: i64, profile_id: i64) -> Self {
        Self {
            deployment_id,
            profile_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<DeploymentAiProviderProfile, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query_as!(
            DeploymentAiProviderProfile,
            r#"
            SELECT
                id, deployment_id, provider, name, slug, api_key, base_url,
                organization, project, default_model, enabled, created_at, updated_at
            FROM deployment_ai_provider_profiles
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.profile_id,
            self.deployment_id
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("AI provider profile not found".to_string()))
    }
}
