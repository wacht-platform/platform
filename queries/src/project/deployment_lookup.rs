use super::*;
pub struct DeploymentByIdRow {
    pub id: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub maintenance_mode: bool,
    pub backend_host: String,
    pub frontend_host: String,
    pub publishable_key: String,
    pub project_id: i64,
    pub mode: String,
    pub mail_from_host: String,
    pub domain_verification_records: Option<serde_json::Value>,
    pub email_verification_records: Option<serde_json::Value>,
    pub email_provider: String,
    pub custom_smtp_config: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct DeploymentByIdQuery {
    deployment_id: Option<i64>,
}

impl DeploymentByIdQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<DeploymentByIdRow, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;

        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deleted_at,
                   maintenance_mode, backend_host, frontend_host, publishable_key,
                   project_id, mode, mail_from_host,
                   domain_verification_records::jsonb as domain_verification_records,
                   email_verification_records::jsonb as email_verification_records,
                   email_provider, custom_smtp_config::jsonb as custom_smtp_config
            FROM deployments
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            deployment_id
        )
        .fetch_one(executor)
        .await?;

        Ok(DeploymentByIdRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            maintenance_mode: row.maintenance_mode,
            backend_host: row.backend_host,
            frontend_host: row.frontend_host,
            publishable_key: row.publishable_key,
            project_id: row.project_id,
            mode: row.mode,
            mail_from_host: row.mail_from_host,
            domain_verification_records: row.domain_verification_records,
            email_verification_records: row.email_verification_records,
            email_provider: row.email_provider,
            custom_smtp_config: row.custom_smtp_config,
        })
    }
}

#[derive(Default)]
pub struct ActiveDeploymentIdsByProjectQuery {
    project_id: Option<i64>,
}

impl ActiveDeploymentIdsByProjectQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let rows = sqlx::query!(
            r#"
            SELECT id FROM deployments
            WHERE project_id = $1 AND deleted_at IS NULL
            "#,
            project_id
        )
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|r| r.id).collect())
    }
}

