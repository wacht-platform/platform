use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::api_key::{ApiAuthApp, RateLimit};

pub struct CreateApiAuthAppCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub rate_limits: Vec<RateLimit>,
}

impl CreateApiAuthAppCommand {
    pub fn new(deployment_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            name,
            description: None,
            rate_limits: vec![],
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_rate_limits(self, rate_limits: Vec<RateLimit>) -> Result<Self, AppError> {
        for limit in &rate_limits {
            limit
                .validate()
                .map_err(|e| AppError::BadRequest(format!("Invalid rate limit: {}", e)))?;
        }

        Ok(Self {
            rate_limits,
            ..self
        })
    }
}

impl Command for CreateApiAuthAppCommand {
    type Output = ApiAuthApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let app_id = app_state.sf.next_id()? as i64;

        let rate_limits_json = serde_json::to_value(&self.rate_limits)
            .map_err(|e| AppError::Internal(format!("Failed to serialize rate limits: {}", e)))?;

        let rec = sqlx::query!(
            r#"
            INSERT INTO api_auth_apps (id, deployment_id, name, description, rate_limits)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING id, deployment_id, name, description, is_active,
                      rate_limits as "rate_limits: serde_json::Value",
                      created_at, updated_at, deleted_at
            "#,
            app_id,
            self.deployment_id,
            self.name,
            self.description,
            rate_limits_json
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(ApiAuthApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            rate_limits: rec
                .rate_limits
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

pub struct UpdateApiAuthAppCommand {
    pub app_id: i64,
    pub deployment_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_active: Option<bool>,
    pub rate_limits: Option<Vec<RateLimit>>,
}

impl Command for UpdateApiAuthAppCommand {
    type Output = ApiAuthApp;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if let Some(ref rate_limits) = self.rate_limits {
            for limit in rate_limits {
                limit
                    .validate()
                    .map_err(|e| AppError::BadRequest(format!("Invalid rate limit: {}", e)))?;
            }
        }

        let rate_limits_json = self
            .rate_limits
            .as_ref()
            .map(|rl| serde_json::to_value(rl))
            .transpose()
            .map_err(|e| AppError::Internal(format!("Failed to serialize rate limits: {}", e)))?;

        let rec = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET
                name = COALESCE($3, name),
                description = COALESCE($4, description),
                is_active = COALESCE($5, is_active),
                rate_limits = COALESCE($6, rate_limits),
                updated_at = NOW()
            WHERE id = $1 AND deployment_id = $2
            RETURNING id, deployment_id, name, description, is_active,
                      rate_limits as "rate_limits: serde_json::Value",
                      created_at, updated_at, deleted_at
            "#,
            self.app_id,
            self.deployment_id,
            self.name,
            self.description,
            self.is_active,
            rate_limits_json
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(ApiAuthApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            rate_limits: rec
                .rate_limits
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        })
    }
}

pub struct DeleteApiAuthAppCommand {
    pub app_id: i64,
    pub deployment_id: i64,
}

impl Command for DeleteApiAuthAppCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            UPDATE api_auth_apps
            SET deleted_at = NOW()
            WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL
            "#,
            self.app_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("API auth app not found".to_string()));
        }

        Ok(())
    }
}
