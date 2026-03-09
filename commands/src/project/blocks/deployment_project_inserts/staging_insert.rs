use super::*;

pub(in crate::project) struct StagingDeploymentInsertedRow {
    pub(in crate::project) id: i64,
    pub(in crate::project) created_at: chrono::DateTime<chrono::Utc>,
    pub(in crate::project) updated_at: chrono::DateTime<chrono::Utc>,
    pub(in crate::project) maintenance_mode: bool,
    pub(in crate::project) backend_host: String,
    pub(in crate::project) frontend_host: String,
    pub(in crate::project) publishable_key: String,
    pub(in crate::project) project_id: i64,
    pub(in crate::project) mode: String,
    pub(in crate::project) mail_from_host: String,
}

#[derive(Default)]
pub(in crate::project) struct StagingDeploymentInsert {
    id: Option<i64>,
    project_id: Option<i64>,
    backend_host: Option<String>,
    frontend_host: Option<String>,
    publishable_key: Option<String>,
    mail_from_host: Option<String>,
}

impl StagingDeploymentInsert {
    pub(in crate::project) fn builder() -> Self {
        Self::default()
    }

    pub(in crate::project) fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub(in crate::project) fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub(in crate::project) fn backend_host(mut self, backend_host: impl Into<String>) -> Self {
        self.backend_host = Some(backend_host.into());
        self
    }

    pub(in crate::project) fn frontend_host(mut self, frontend_host: impl Into<String>) -> Self {
        self.frontend_host = Some(frontend_host.into());
        self
    }

    pub(in crate::project) fn publishable_key(mut self, publishable_key: impl Into<String>) -> Self {
        self.publishable_key = Some(publishable_key.into());
        self
    }

    pub(in crate::project) fn mail_from_host(mut self, mail_from_host: impl Into<String>) -> Self {
        self.mail_from_host = Some(mail_from_host.into());
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<StagingDeploymentInsertedRow, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = self
            .id
            .ok_or_else(|| AppError::Validation("staging deployment id is required".to_string()))?;
        let project_id = self.project_id.ok_or_else(|| {
            AppError::Validation("staging deployment project_id is required".to_string())
        })?;
        let backend_host = self.backend_host.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment backend_host is required".to_string())
        })?;
        let frontend_host = self.frontend_host.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment frontend_host is required".to_string())
        })?;
        let publishable_key = self.publishable_key.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment publishable_key is required".to_string())
        })?;
        let mail_from_host = self.mail_from_host.as_deref().ok_or_else(|| {
            AppError::Validation("staging deployment mail_from_host is required".to_string())
        })?;

        let now = chrono::Utc::now();

        let row = sqlx::query!(
            r#"
            INSERT INTO deployments (
                id,
                project_id,
                mode,
                backend_host,
                frontend_host,
                publishable_key,
                maintenance_mode,
                mail_from_host,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, created_at, updated_at, deleted_at,
                     maintenance_mode, backend_host, frontend_host, publishable_key, project_id, mode, mail_from_host
            "#,
            id,
            project_id,
            "staging",
            backend_host,
            frontend_host,
            publishable_key,
            false,
            mail_from_host,
            now,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(StagingDeploymentInsertedRow {
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
        })
    }
}
