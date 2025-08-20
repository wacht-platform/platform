use crate::Command;
use common::error::AppError;
use models::api_key::ApiKeyApp;
use common::state::AppState;

pub struct CreateApiKeyAppCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub rate_limit_per_minute: Option<i32>,
    pub rate_limit_per_hour: Option<i32>,
    pub rate_limit_per_day: Option<i32>,
}

impl CreateApiKeyAppCommand {
    pub fn new(deployment_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            name,
            description: None,
            rate_limit_per_minute: None,
            rate_limit_per_hour: None,
            rate_limit_per_day: None,
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_rate_limits(mut self, per_minute: i32, per_hour: i32, per_day: i32) -> Self {
        self.rate_limit_per_minute = Some(per_minute);
        self.rate_limit_per_hour = Some(per_hour);
        self.rate_limit_per_day = Some(per_day);
        self
    }

    pub fn with_rate_limit_per_minute(mut self, per_minute: i32) -> Self {
        self.rate_limit_per_minute = Some(per_minute);
        self
    }

    pub fn with_rate_limit_per_hour(mut self, per_hour: i32) -> Self {
        self.rate_limit_per_hour = Some(per_hour);
        self
    }

    pub fn with_rate_limit_per_day(mut self, per_day: i32) -> Self {
        self.rate_limit_per_day = Some(per_day);
        self
    }
}

impl Command for CreateApiKeyAppCommand {
    type Output = ApiKeyApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Generate Snowflake ID
        let app_id = app_state.sf.next_id()? as i64;
        
        let rec = sqlx::query!(
            r#"
            INSERT INTO api_key_apps (id, deployment_id, name, description, rate_limit_per_minute, rate_limit_per_hour, rate_limit_per_day)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, deployment_id, name, description, is_active, rate_limit_per_minute, rate_limit_per_hour, rate_limit_per_day, created_at, updated_at, deleted_at
            "#,
            app_id,
            self.deployment_id,
            self.name,
            self.description,
            self.rate_limit_per_minute,
            self.rate_limit_per_hour,
            self.rate_limit_per_day
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(ApiKeyApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            rate_limit_per_minute: rec.rate_limit_per_minute,
            rate_limit_per_hour: rec.rate_limit_per_hour,
            rate_limit_per_day: rec.rate_limit_per_day,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

pub struct UpdateApiKeyAppCommand {
    pub app_id: i64,
    pub deployment_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limit_per_minute: Option<i32>,
    pub rate_limit_per_hour: Option<i32>,
    pub rate_limit_per_day: Option<i32>,
}

impl Command for UpdateApiKeyAppCommand {
    type Output = ApiKeyApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            r#"
            UPDATE api_key_apps
            SET 
                name = COALESCE($3, name),
                description = COALESCE($4, description),
                is_active = COALESCE($5, is_active),
                rate_limit_per_minute = COALESCE($6, rate_limit_per_minute),
                rate_limit_per_hour = COALESCE($7, rate_limit_per_hour),
                rate_limit_per_day = COALESCE($8, rate_limit_per_day),
                updated_at = NOW()
            WHERE id = $1 AND deployment_id = $2
            RETURNING id, deployment_id, name, description, is_active, rate_limit_per_minute, rate_limit_per_hour, rate_limit_per_day, created_at, updated_at, deleted_at
            "#,
            self.app_id,
            self.deployment_id,
            self.name,
            self.description,
            self.is_active,
            self.rate_limit_per_minute,
            self.rate_limit_per_hour,
            self.rate_limit_per_day
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(ApiKeyApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            rate_limit_per_minute: rec.rate_limit_per_minute,
            rate_limit_per_hour: rec.rate_limit_per_hour,
            rate_limit_per_day: rec.rate_limit_per_day,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

pub struct DeleteApiKeyAppCommand {
    pub app_id: i64,
    pub deployment_id: i64,
}

impl Command for DeleteApiKeyAppCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            UPDATE api_key_apps
            SET deleted_at = NOW()
            WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL
            "#,
            self.app_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("API key app not found".to_string()));
        }

        Ok(())
    }
}