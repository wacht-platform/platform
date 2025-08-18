use serde::Deserialize;
use sqlx::{query, query_as};

use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::{WebhookApp, WebhookAppEvent, webhook::WebhookEventDefinition};

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
    pub description: Option<String>,
    pub events: Vec<WebhookEventDefinition>,
}

impl CreateWebhookAppCommand {
    pub fn new(deployment_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            name,
            description: None,
            events: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_events(mut self, events: Vec<WebhookEventDefinition>) -> Self {
        self.events = events;
        self
    }
}

impl Command for CreateWebhookAppCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await?;

        let signing_secret = generate_signing_secret();

        let app = query_as!(
            WebhookApp,
            r#"
            INSERT INTO webhook_apps (deployment_id, name, description, signing_secret, is_active)
            VALUES ($1, $2, $3, $4, true)
            RETURNING deployment_id as "deployment_id!",
                      name as "name!",
                      description,
                      signing_secret as "signing_secret!",
                      is_active as "is_active!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.name,
            self.description,
            signing_secret
        )
        .fetch_one(&mut *tx)
        .await?;

        for event in self.events {
            query!(
                r#"
                INSERT INTO webhook_app_events (deployment_id, app_name, event_name, description, schema)
                VALUES ($1, $2, $3, $4, $5)
                "#,
                self.deployment_id,
                app.name.clone(),
                event.name,
                event.description,
                event.schema
            )
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(app)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateWebhookAppCommand {
    pub deployment_id: i64,
    pub app_name: String,
    pub new_name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
}

impl Command for UpdateWebhookAppCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let app = query_as!(
            WebhookApp,
            r#"
            UPDATE webhook_apps
            SET name = COALESCE($3, name),
                description = COALESCE($4, description),
                is_active = COALESCE($5, is_active),
                updated_at = NOW()
            WHERE deployment_id = $1 AND name = $2
            RETURNING deployment_id as "deployment_id!",
                      name as "name!",
                      description,
                      signing_secret as "signing_secret!",
                      is_active as "is_active!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.app_name,
            self.new_name,
            self.description,
            self.is_active
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook app not found".to_string()))?;

        Ok(app)
    }
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebhookAppCommand {
    pub deployment_id: i64,
    pub app_name: String,
}

impl Command for DeleteWebhookAppCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = query!(
            r#"
            DELETE FROM webhook_apps
            WHERE deployment_id = $1 AND name = $2
            "#,
            self.deployment_id,
            self.app_name
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Webhook app not found".to_string()));
        }

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct AddWebhookEventCommand {
    pub deployment_id: i64,
    pub app_name: String,
    pub event: WebhookEventDefinition,
}

impl Command for AddWebhookEventCommand {
    type Output = WebhookAppEvent;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Verify app exists and belongs to deployment
        let app_exists = query!(
            r#"
            SELECT 1 as exists
            FROM webhook_apps
            WHERE deployment_id = $1 AND name = $2
            "#,
            self.deployment_id,
            self.app_name
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .is_some();

        if !app_exists {
            return Err(AppError::NotFound("Webhook app not found".to_string()));
        }

        let event = query_as!(
            WebhookAppEvent,
            r#"
            INSERT INTO webhook_app_events (deployment_id, app_name, event_name, description, schema)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING deployment_id as "deployment_id!",
                      app_name as "app_name!",
                      event_name as "event_name!",
                      description,
                      schema,
                      created_at as "created_at!"
            "#,
            self.deployment_id,
            self.app_name,
            self.event.name,
            self.event.description,
            self.event.schema
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(event)
    }
}

#[derive(Debug, Deserialize)]
pub struct RotateWebhookSecretCommand {
    pub deployment_id: i64,
    pub app_name: String,
}

impl Command for RotateWebhookSecretCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let new_secret = generate_signing_secret();

        let app = query_as!(
            WebhookApp,
            r#"
            UPDATE webhook_apps
            SET signing_secret = $3, updated_at = NOW()
            WHERE deployment_id = $1 AND name = $2
            RETURNING 
                deployment_id as "deployment_id!",
                name as "name!",
                description,
                signing_secret as "signing_secret!",
                is_active as "is_active!",
                created_at as "created_at!",
                updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.app_name,
            new_secret
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Webhook app not found".to_string()))?;

        Ok(app)
    }
}
