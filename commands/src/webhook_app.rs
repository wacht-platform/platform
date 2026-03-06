use serde::Deserialize;
use sqlx::{query, query_as};

use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::WebhookApp;

fn generate_signing_secret() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();

    use base64::{Engine, engine::general_purpose::STANDARD};
    format!("whsec_{}", STANDARD.encode(bytes))
}

#[derive(Debug, Deserialize)]
pub struct CreateWebhookAppCommand {
    pub deployment_id: i64,
    pub name: String,
    pub app_slug: Option<String>,
    pub description: Option<String>,
    pub failure_notification_emails: Option<Vec<String>>,
    pub event_catalog_slug: Option<String>, // Added for shared event catalogs
}

impl CreateWebhookAppCommand {
    pub fn new(deployment_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            name,
            app_slug: None,
            description: None,
            failure_notification_emails: None,
            event_catalog_slug: None,
        }
    }

    pub fn with_app_slug(mut self, app_slug: String) -> Self {
        self.app_slug = Some(app_slug);
        self
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_failure_notification_emails(mut self, emails: Vec<String>) -> Self {
        self.failure_notification_emails = Some(emails);
        self
    }

    pub fn with_event_catalog_slug(mut self, slug: String) -> Self {
        self.event_catalog_slug = Some(slug);
        self
    }
}

impl Command for CreateWebhookAppCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(
            app_state.db_router.writer(),
            format!("slug_{}", app_state.sf.next_id()?),
        )
        .await
    }
}

impl CreateWebhookAppCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        generated_slug: String,
    ) -> Result<WebhookApp, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let signing_secret = generate_signing_secret();

        // Generate app_slug: always use "slug_" prefix
        let app_slug = if let Some(slug) = self.app_slug {
            slug
        } else {
            generated_slug
        };

        let app = query_as!(
            WebhookApp,
            r#"
            INSERT INTO webhook_apps (deployment_id, app_slug, name, description, signing_secret, failure_notification_emails, event_catalog_slug, is_active)
            VALUES ($1, $2, $3, $4, $5, $6, $7, true)
            RETURNING deployment_id as "deployment_id!",
                      app_slug as "app_slug!",
                      name as "name!",
                      description,
                      signing_secret as "signing_secret!",
                      failure_notification_emails,
                      event_catalog_slug,
                      is_active as "is_active!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            app_slug,
            self.name,
            self.description,
            signing_secret,
            serde_json::to_value(self.failure_notification_emails.unwrap_or_default())
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            self.event_catalog_slug
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(app)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookAppCommand {
    pub deployment_id: i64,
    pub app_slug: String,
    pub new_name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub failure_notification_emails: Option<Vec<String>>,
    pub event_catalog_slug: Option<String>,
}

impl Command for UpdateWebhookAppCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl UpdateWebhookAppCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<WebhookApp, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let app: Option<WebhookApp> = query_as!(
            WebhookApp,
            r#"
            UPDATE webhook_apps
            SET name = COALESCE($3, name),
                description = COALESCE($4, description),
                is_active = COALESCE($5, is_active),
                failure_notification_emails = COALESCE($6, failure_notification_emails),
                event_catalog_slug = COALESCE($7, event_catalog_slug),
                updated_at = NOW()
            WHERE deployment_id = $1 AND app_slug = $2
            RETURNING deployment_id as "deployment_id!",
                      app_slug as "app_slug!",
                      name as "name!",
                      description,
                      signing_secret as "signing_secret!",
                      failure_notification_emails,
                      event_catalog_slug,
                      is_active as "is_active!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.app_slug,
            self.new_name,
            self.description,
            self.is_active,
            self.failure_notification_emails
                .map(|emails| serde_json::to_value(emails))
                .transpose()
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            self.event_catalog_slug
        )
        .fetch_optional(&mut *conn)
        .await?;

        let app = app.ok_or_else(|| AppError::NotFound("Webhook app not found".to_string()))?;

        Ok(app)
    }
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebhookAppCommand {
    pub deployment_id: i64,
    pub app_slug: String,
}

impl Command for DeleteWebhookAppCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl DeleteWebhookAppCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = query!(
            r#"
            DELETE FROM webhook_apps
            WHERE deployment_id = $1 AND app_slug = $2
            "#,
            self.deployment_id,
            self.app_slug
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Webhook app not found".to_string()));
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct RotateWebhookSecretCommand {
    pub deployment_id: i64,
    pub app_slug: String,
}

impl Command for RotateWebhookSecretCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl RotateWebhookSecretCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<WebhookApp, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let new_secret = generate_signing_secret();

        let app: Option<WebhookApp> = query_as!(
            WebhookApp,
            r#"
            UPDATE webhook_apps
            SET signing_secret = $3, updated_at = NOW()
            WHERE deployment_id = $1 AND app_slug = $2
            RETURNING
                deployment_id as "deployment_id!",
                app_slug as "app_slug!",
                name as "name!",
                description,
                signing_secret as "signing_secret!",
                failure_notification_emails,
                event_catalog_slug,
                is_active as "is_active!",
                created_at as "created_at!",
                updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.app_slug,
            new_secret
        )
        .fetch_optional(&mut *conn)
        .await?;

        let app = app.ok_or_else(|| AppError::NotFound("Webhook app not found".to_string()))?;

        Ok(app)
    }
}
