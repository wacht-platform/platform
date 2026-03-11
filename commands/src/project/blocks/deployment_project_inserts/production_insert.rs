use super::*;

pub(in crate::project) struct ProductionDeploymentInsertedRow {
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
    pub(in crate::project) email_provider: String,
    pub(in crate::project) custom_smtp_config: Option<serde_json::Value>,
}
#[derive(Default)]
pub(in crate::project) struct ProductionDeploymentInsert {
    id: Option<i64>,
    project_id: Option<i64>,
    backend_host: Option<String>,
    frontend_host: Option<String>,
    publishable_key: Option<String>,
    mail_from_host: Option<String>,
    domain_verification_records: Option<serde_json::Value>,
    email_verification_records: Option<serde_json::Value>,
}

impl ProductionDeploymentInsert {
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

    pub(in crate::project) fn domain_verification_records(
        mut self,
        domain_verification_records: serde_json::Value,
    ) -> Self {
        self.domain_verification_records = Some(domain_verification_records);
        self
    }

    pub(in crate::project) fn email_verification_records(
        mut self,
        email_verification_records: serde_json::Value,
    ) -> Self {
        self.email_verification_records = Some(email_verification_records);
        self
    }

    pub(in crate::project) async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<ProductionDeploymentInsertedRow, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        const SCOPE: &str = "production deployment";
        let id = required_i64(self.id, SCOPE, "id")?;
        let project_id = required_i64(self.project_id, SCOPE, "project_id")?;
        let backend_host = required_str(self.backend_host.as_ref(), SCOPE, "backend_host")?;
        let frontend_host = required_str(self.frontend_host.as_ref(), SCOPE, "frontend_host")?;
        let publishable_key =
            required_str(self.publishable_key.as_ref(), SCOPE, "publishable_key")?;
        let mail_from_host = required_str(self.mail_from_host.as_ref(), SCOPE, "mail_from_host")?;
        let domain_verification_records = required_json(
            self.domain_verification_records.as_ref(),
            SCOPE,
            "domain_verification_records",
        )?;
        let email_verification_records = required_json(
            self.email_verification_records.as_ref(),
            SCOPE,
            "email_verification_records",
        )?;

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
                domain_verification_records,
                email_verification_records,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
            RETURNING id, created_at, updated_at, deleted_at,
                     maintenance_mode, backend_host, frontend_host, publishable_key, project_id, mode, mail_from_host,
                     email_provider, custom_smtp_config::jsonb as custom_smtp_config
            "#,
            id,
            project_id,
            "production",
            backend_host,
            frontend_host,
            publishable_key,
            false,
            mail_from_host,
            domain_verification_records,
            email_verification_records,
            now,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(ProductionDeploymentInsertedRow {
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
            email_provider: row.email_provider,
            custom_smtp_config: row.custom_smtp_config,
        })
    }
}
