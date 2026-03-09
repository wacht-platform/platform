use super::*;
pub(in crate::project) struct DeploymentAiSettingsInsert {
    id: i64,
    deployment_id: i64,
}

#[derive(Default)]
pub(in crate::project) struct DeploymentAiSettingsInsertBuilder {
    id: Option<i64>,
    deployment_id: Option<i64>,
}

impl DeploymentAiSettingsInsert {
    pub(in crate::project) fn builder() -> DeploymentAiSettingsInsertBuilder {
        DeploymentAiSettingsInsertBuilder::default()
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(&self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO deployment_ai_settings (id, deployment_id, created_at, updated_at)
            VALUES ($1, $2, $3, $4)
            "#,
            self.id,
            self.deployment_id,
            now,
            now,
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

impl DeploymentAiSettingsInsertBuilder {
    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub(in crate::project) fn build(self) -> Result<DeploymentAiSettingsInsert, AppError> {
        let id = self.id.ok_or_else(|| {
            AppError::Validation("deployment_ai_settings insert id is required".to_string())
        })?;
        let deployment_id = self.deployment_id.ok_or_else(|| {
            AppError::Validation("deployment_ai_settings deployment_id is required".to_string())
        })?;

        Ok(DeploymentAiSettingsInsert { id, deployment_id })
    }
}

