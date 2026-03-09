use super::*;
pub(in crate::project) struct ConsoleAppBootstrapInsert {
    console_deployment_id: i64,
    target_deployment_id: i64,
    event_catalog_slug: String,
}

#[derive(Default)]
pub(in crate::project) struct ConsoleAppBootstrapInsertBuilder {
    console_deployment_id: Option<i64>,
    target_deployment_id: Option<i64>,
    event_catalog_slug: Option<String>,
}

impl ConsoleAppBootstrapInsert {
    pub(in crate::project) fn builder() -> ConsoleAppBootstrapInsertBuilder {
        ConsoleAppBootstrapInsertBuilder::default()
    }

    pub(in crate::project) async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres> + Send,
    {
        let mut tx = acquirer.begin().await?;
        let app_name = self.target_deployment_id.to_string();
        let now = chrono::Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (deployment_id, app_slug, name, description, is_active, created_at, updated_at, key_prefix)
            VALUES ($1, $2, $3, $4, true, $5, $6, 'sk_')
            "#,
            self.console_deployment_id,
            format!("aa_{}", self.target_deployment_id),
            &app_name,
            format!("API keys for deployment {}", self.target_deployment_id),
            now,
            now,
        )
        .execute(tx.as_mut())
        .await?;

        let signing_secret = generate_signing_secret();

        sqlx::query!(
            r#"
            INSERT INTO webhook_apps (deployment_id, name, description, signing_secret, event_catalog_slug, is_active, created_at, updated_at, app_slug)
            VALUES ($1, $2, $3, $4, $5, true, $6, $7, $8)
            "#,
            self.console_deployment_id,
            &app_name,
            format!("Webhooks for deployment {}", self.target_deployment_id),
            signing_secret,
            &self.event_catalog_slug,
            now,
            now,
            format!("wh_{}", self.target_deployment_id)
        )
        .execute(tx.as_mut())
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

impl ConsoleAppBootstrapInsertBuilder {
    pub(in crate::project) fn console_deployment_id(mut self, console_deployment_id: i64) -> Self {
        self.console_deployment_id = Some(console_deployment_id);
        self
    }

    pub(in crate::project) fn target_deployment_id(mut self, target_deployment_id: i64) -> Self {
        self.target_deployment_id = Some(target_deployment_id);
        self
    }

    pub(in crate::project) fn event_catalog_slug(mut self, event_catalog_slug: impl Into<String>) -> Self {
        self.event_catalog_slug = Some(event_catalog_slug.into());
        self
    }

    pub(in crate::project) fn build(self) -> Result<ConsoleAppBootstrapInsert, AppError> {
        let console_deployment_id = self
            .console_deployment_id
            .ok_or_else(|| AppError::Validation("console deployment id is required".to_string()))?;
        let target_deployment_id = self
            .target_deployment_id
            .ok_or_else(|| AppError::Validation("target deployment id is required".to_string()))?;

        Ok(ConsoleAppBootstrapInsert {
            console_deployment_id,
            target_deployment_id,
            event_catalog_slug: self
                .event_catalog_slug
                .unwrap_or_else(|| DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG.to_string()),
        })
    }
}

