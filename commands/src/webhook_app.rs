use serde::Deserialize;
use sqlx::{query, query_as};

use crate::Command;
use common::error::AppError;
use models::{webhook::WebhookEventDefinition, WebhookApp, WebhookAppEvent};
use common::state::AppState;



fn generate_signing_secret() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    
    use base64::{engine::general_purpose::STANDARD, Engine};
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

        // Create webhook app
        let app = query_as!(
            WebhookApp,
            r#"
            INSERT INTO webhook_apps (deployment_id, name, description, signing_secret, is_active, rate_limit_per_minute)
            VALUES ($1, $2, $3, $4, true, 60)
            RETURNING id as "id!", 
                      deployment_id as "deployment_id!", 
                      name as "name!", 
                      description, 
                      signing_secret as "signing_secret!", 
                      is_active as "is_active!", 
                      rate_limit_per_minute as "rate_limit_per_minute!", 
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

        // Create events
        for event in self.events {
            query!(
                r#"
                INSERT INTO webhook_app_events (app_id, event_name, description, schema)
                VALUES ($1, $2, $3, $4)
                "#,
                app.id,
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
    pub app_id: i64,
    pub deployment_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_per_minute: Option<i32>,
}

impl Command for UpdateWebhookAppCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Build dynamic update query
        let mut tx = app_state.db_pool.begin().await?;
        
        // First check if app exists
        let exists = query!(
            "SELECT 1 as exists FROM webhook_apps WHERE id = $1 AND deployment_id = $2",
            self.app_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?
        .is_some();
        
        if !exists {
            return Err(AppError::NotFound("Webhook app not found".to_string()));
        }
        
        // Update fields that are provided
        if let Some(name) = self.name {
            query!(
                "UPDATE webhook_apps SET name = $1 WHERE id = $2",
                name,
                self.app_id
            )
            .execute(&mut *tx)
            .await?;
        }
        
        if let Some(description) = self.description {
            query!(
                "UPDATE webhook_apps SET description = $1 WHERE id = $2",
                description,
                self.app_id
            )
            .execute(&mut *tx)
            .await?;
        }
        
        if let Some(is_active) = self.is_active {
            query!(
                "UPDATE webhook_apps SET is_active = $1 WHERE id = $2",
                is_active,
                self.app_id
            )
            .execute(&mut *tx)
            .await?;
        }
        
        if let Some(rate_limit) = self.rate_limit_per_minute {
            query!(
                "UPDATE webhook_apps SET rate_limit_per_minute = $1 WHERE id = $2",
                rate_limit,
                self.app_id
            )
            .execute(&mut *tx)
            .await?;
        }
        
        // Fetch updated app
        let app = query_as!(
            WebhookApp,
            r#"
            SELECT id as "id!", 
                   deployment_id as "deployment_id!", 
                   name as "name!", 
                   description, 
                   signing_secret as "signing_secret!",
                   is_active as "is_active!", 
                   rate_limit_per_minute as "rate_limit_per_minute!", 
                   created_at as "created_at!", 
                   updated_at as "updated_at!"
            FROM webhook_apps
            WHERE id = $1
            "#,
            self.app_id
        )
        .fetch_one(&mut *tx)
        .await?;
        
        tx.commit().await?;
        Ok(app)
    }
}

#[derive(Debug, Deserialize)]
pub struct DeleteWebhookAppCommand {
    pub app_id: i64,
    pub deployment_id: i64,
}

impl Command for DeleteWebhookAppCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = query!(
            r#"
            DELETE FROM webhook_apps
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.app_id,
            self.deployment_id
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
    pub app_id: i64,
    pub deployment_id: i64,
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
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.app_id,
            self.deployment_id
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
            INSERT INTO webhook_app_events (app_id, event_name, description, schema)
            VALUES ($1, $2, $3, $4)
            RETURNING id as "id!", 
                      app_id as "app_id!", 
                      event_name as "event_name!", 
                      description, 
                      schema, 
                      created_at as "created_at!"
            "#,
            self.app_id,
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
    pub app_id: i64,
    pub deployment_id: i64,
}

impl Command for RotateWebhookSecretCommand {
    type Output = WebhookApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let new_secret = generate_signing_secret();

        let result = query!(
            r#"
            UPDATE webhook_apps
            SET signing_secret = $3, updated_at = NOW()
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.app_id,
            self.deployment_id,
            new_secret
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Webhook app not found".to_string()));
        }

        // Fetch and return the updated app
        let app = query_as!(
            WebhookApp,
            r#"
            SELECT id as "id!", 
                   deployment_id as "deployment_id!", 
                   name as "name!", 
                   description, 
                   signing_secret as "signing_secret!",
                   is_active as "is_active!", 
                   rate_limit_per_minute as "rate_limit_per_minute!", 
                   created_at as "created_at!", 
                   updated_at as "updated_at!"
            FROM webhook_apps
            WHERE id = $1
            "#,
            self.app_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(app)
    }
}