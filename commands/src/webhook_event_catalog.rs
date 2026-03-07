use serde::Deserialize;
use sqlx::Connection;
use sqlx::query_as;

use common::error::AppError;
use models::webhook::{WebhookEventCatalog, WebhookEventDefinition};

#[derive(Debug, Deserialize)]
pub struct CreateEventCatalogCommand {
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub events: Vec<WebhookEventDefinition>,
}

impl CreateEventCatalogCommand {
    pub fn new(
        deployment_id: i64,
        slug: String,
        name: String,
        events: Vec<WebhookEventDefinition>,
    ) -> Self {
        Self {
            deployment_id,
            slug,
            name,
            description: None,
            events,
        }
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }
}

impl CreateEventCatalogCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<WebhookEventCatalog, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let events_json = serde_json::to_value(&self.events)
            .map_err(|e| AppError::BadRequest(format!("Invalid events format: {}", e)))?;

        let catalog = query_as!(
            WebhookEventCatalog,
            r#"
            INSERT INTO webhook_event_catalogs (deployment_id, slug, name, description, events)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING deployment_id as "deployment_id!",
                      slug as "slug!",
                      name as "name!",
                      description,
                      events as "events!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.slug,
            self.name,
            self.description,
            events_json
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(catalog)
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateEventCatalogCommand {
    pub deployment_id: i64,
    pub slug: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl UpdateEventCatalogCommand {
    pub fn new(deployment_id: i64, slug: String) -> Self {
        Self {
            deployment_id,
            slug,
            name: None,
            description: None,
        }
    }

    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }
}

impl UpdateEventCatalogCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<WebhookEventCatalog, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let catalog = query_as!(
            WebhookEventCatalog,
            r#"
            UPDATE webhook_event_catalogs
            SET name = COALESCE($3, name),
                description = COALESCE($4, description),
                updated_at = NOW()
            WHERE deployment_id = $1 AND slug = $2
            RETURNING deployment_id as "deployment_id!",
                      slug as "slug!",
                      name as "name!",
                      description,
                      events as "events!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.slug,
            self.name,
            self.description
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("Event catalog not found".to_string()))?;

        Ok(catalog)
    }
}

#[derive(Debug, Deserialize)]
pub struct AppendEventsToCatalogCommand {
    pub deployment_id: i64,
    pub slug: String,
    pub events: Vec<WebhookEventDefinition>,
}

impl AppendEventsToCatalogCommand {
    pub fn new(deployment_id: i64, slug: String, events: Vec<WebhookEventDefinition>) -> Self {
        Self {
            deployment_id,
            slug,
            events,
        }
    }
}

impl AppendEventsToCatalogCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<WebhookEventCatalog, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut tx = conn.begin().await?;

        let catalog = query_as!(
            WebhookEventCatalog,
            r#"
            SELECT deployment_id as "deployment_id!",
                   slug as "slug!",
                   name as "name!",
                   description,
                   events as "events!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_event_catalogs
            WHERE deployment_id = $1 AND slug = $2
            FOR UPDATE
            "#,
            self.deployment_id,
            self.slug
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Event catalog not found".to_string()))?;

        let mut current_events: Vec<WebhookEventDefinition> =
            serde_json::from_value(catalog.events).map_err(|e| {
                AppError::Internal(format!("Failed to parse current events: {}", e))
            })?;

        for new_event in self.events {
            if current_events.iter().any(|e| e.name == new_event.name) {
                return Err(AppError::Validation(format!(
                    "Event with name '{}' already exists in the catalog",
                    new_event.name
                )));
            }
            current_events.push(new_event);
        }

        let events_json = serde_json::to_value(&current_events).map_err(|e| {
            AppError::Internal(format!("Failed to serialize updated events: {}", e))
        })?;

        let updated_catalog = query_as!(
            WebhookEventCatalog,
            r#"
            UPDATE webhook_event_catalogs
            SET events = $3, updated_at = NOW()
            WHERE deployment_id = $1 AND slug = $2
            RETURNING deployment_id as "deployment_id!",
                      slug as "slug!",
                      name as "name!",
                      description,
                      events as "events!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.slug,
            events_json
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(updated_catalog)
    }
}

#[derive(Debug, Deserialize)]
pub struct ArchiveEventInCatalogCommand {
    pub deployment_id: i64,
    pub slug: String,
    pub event_name: String,
    pub is_archived: bool,
}

impl ArchiveEventInCatalogCommand {
    pub fn new(deployment_id: i64, slug: String, event_name: String, is_archived: bool) -> Self {
        Self {
            deployment_id,
            slug,
            event_name,
            is_archived,
        }
    }
}

impl ArchiveEventInCatalogCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<WebhookEventCatalog, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut tx = conn.begin().await?;

        let catalog = query_as!(
            WebhookEventCatalog,
            r#"
            SELECT deployment_id as "deployment_id!",
                   slug as "slug!",
                   name as "name!",
                   description,
                   events as "events!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_event_catalogs
            WHERE deployment_id = $1 AND slug = $2
            FOR UPDATE
            "#,
            self.deployment_id,
            self.slug
        )
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound("Event catalog not found".to_string()))?;

        let mut current_events: Vec<WebhookEventDefinition> =
            serde_json::from_value(catalog.events).map_err(|e| {
                AppError::Internal(format!("Failed to parse current events: {}", e))
            })?;

        let mut found = false;
        for event in &mut current_events {
            if event.name == self.event_name {
                event.is_archived = self.is_archived;
                found = true;
                break;
            }
        }

        if !found {
            return Err(AppError::NotFound(format!(
                "Event with name '{}' not found in the catalog",
                self.event_name
            )));
        }

        let events_json = serde_json::to_value(&current_events).map_err(|e| {
            AppError::Internal(format!("Failed to serialize updated events: {}", e))
        })?;

        let updated_catalog = query_as!(
            WebhookEventCatalog,
            r#"
            UPDATE webhook_event_catalogs
            SET events = $3, updated_at = NOW()
            WHERE deployment_id = $1 AND slug = $2
            RETURNING deployment_id as "deployment_id!",
                      slug as "slug!",
                      name as "name!",
                      description,
                      events as "events!",
                      created_at as "created_at!",
                      updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.slug,
            events_json
        )
        .fetch_one(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(updated_catalog)
    }
}

#[derive(Debug, Deserialize)]
pub struct GetEventCatalogQuery {
    pub deployment_id: i64,
    pub slug: String,
}

impl GetEventCatalogQuery {
    pub fn new(deployment_id: i64, slug: String) -> Self {
        Self {
            deployment_id,
            slug,
        }
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
    ) -> Result<Option<WebhookEventCatalog>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let catalog = query_as!(
            WebhookEventCatalog,
            r#"
            SELECT deployment_id as "deployment_id!",
                   slug as "slug!",
                   name as "name!",
                   description,
                   events as "events!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_event_catalogs
            WHERE deployment_id = $1 AND slug = $2
            "#,
            self.deployment_id,
            self.slug
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(catalog)
    }
}

#[derive(Debug, Deserialize)]
pub struct ListEventCatalogsQuery {
    pub deployment_id: i64,
}

impl ListEventCatalogsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
    ) -> Result<Vec<WebhookEventCatalog>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let catalogs = query_as!(
            WebhookEventCatalog,
            r#"
            SELECT deployment_id as "deployment_id!",
                   slug as "slug!",
                   name as "name!",
                   description,
                   events as "events!",
                   created_at as "created_at!",
                   updated_at as "updated_at!"
            FROM webhook_event_catalogs
            WHERE deployment_id = $1
            ORDER BY name ASC
            "#,
            self.deployment_id
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(catalogs)
    }
}
