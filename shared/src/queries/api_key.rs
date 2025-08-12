use crate::{error::AppError, models::api_key::{ApiKeyApp, ApiKey}, state::AppState};
use super::Query;

pub struct GetApiKeyAppsQuery {
    pub deployment_id: i64,
    pub include_inactive: bool,
}

impl GetApiKeyAppsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            include_inactive: false,
        }
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }
}

impl Query for GetApiKeyAppsQuery {
    type Output = Vec<ApiKeyApp>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let apps = if self.include_inactive {
            let recs = sqlx::query!(
                "SELECT * FROM api_key_apps WHERE deployment_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await?;
            
            recs.into_iter().map(|rec| ApiKeyApp {
                id: rec.id,
                deployment_id: rec.deployment_id,
                name: rec.name,
                description: rec.description,
                is_active: rec.is_active.unwrap_or(true),
                rate_limit_per_minute: rec.rate_limit_per_minute,
                rate_limit_per_hour: rec.rate_limit_per_hour,
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                deleted_at: rec.deleted_at,
            }).collect()
        } else {
            let recs = sqlx::query!(
                "SELECT * FROM api_key_apps WHERE deployment_id = $1 AND is_active = true AND deleted_at IS NULL ORDER BY created_at DESC",
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await?;
            
            recs.into_iter().map(|rec| ApiKeyApp {
                id: rec.id,
                deployment_id: rec.deployment_id,
                name: rec.name,
                description: rec.description,
                is_active: rec.is_active.unwrap_or(true),
                rate_limit_per_minute: rec.rate_limit_per_minute,
                rate_limit_per_hour: rec.rate_limit_per_hour,
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                deleted_at: rec.deleted_at,
            }).collect()
        };

        Ok(apps)
    }
}

pub struct GetApiKeyAppByIdQuery {
    pub app_id: i64,
    pub deployment_id: i64,
}

impl Query for GetApiKeyAppByIdQuery {
    type Output = Option<ApiKeyApp>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            "SELECT * FROM api_key_apps WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
            self.app_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(rec.map(|rec| ApiKeyApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            rate_limit_per_minute: rec.rate_limit_per_minute,
            rate_limit_per_hour: rec.rate_limit_per_hour,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        }))
    }
}

pub struct GetApiKeyAppByNameQuery {
    pub deployment_id: i64,
    pub name: String,
}

impl GetApiKeyAppByNameQuery {
    pub fn new(deployment_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            name,
        }
    }
}

impl Query for GetApiKeyAppByNameQuery {
    type Output = Option<ApiKeyApp>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            "SELECT * FROM api_key_apps WHERE deployment_id = $1 AND name = $2 AND deleted_at IS NULL",
            self.deployment_id,
            self.name
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(rec.map(|rec| ApiKeyApp {
            id: rec.id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            description: rec.description,
            is_active: rec.is_active.unwrap_or(true),
            rate_limit_per_minute: rec.rate_limit_per_minute,
            rate_limit_per_hour: rec.rate_limit_per_hour,
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            deleted_at: rec.deleted_at,
        }))
    }
}

pub struct GetApiKeysByAppQuery {
    pub app_id: i64,
    pub deployment_id: i64,
    pub include_inactive: bool,
}

impl GetApiKeysByAppQuery {
    pub fn new(app_id: i64, deployment_id: i64) -> Self {
        Self {
            app_id,
            deployment_id,
            include_inactive: false,
        }
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }
}

impl Query for GetApiKeysByAppQuery {
    type Output = Vec<ApiKey>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let keys = if self.include_inactive {
            let recs = sqlx::query!(
                r#"SELECT id, app_id, deployment_id, name, key_prefix, key_suffix, key_hash,
                   permissions as "permissions: serde_json::Value",
                   metadata as "metadata: serde_json::Value",
                   expires_at, last_used_at, is_active, created_at, updated_at,
                   revoked_at, revoked_reason
                   FROM api_keys WHERE app_id = $1 AND deployment_id = $2 ORDER BY created_at DESC"#,
                self.app_id,
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await?;
            
            recs.into_iter().map(|rec| ApiKey {
                id: rec.id,
                app_id: rec.app_id,
                deployment_id: rec.deployment_id,
                name: rec.name,
                key_prefix: rec.key_prefix,
                key_suffix: rec.key_suffix,
                key_hash: rec.key_hash,
                permissions: serde_json::from_value(rec.permissions.clone().unwrap_or(serde_json::json!([]))).unwrap_or_default(),
                metadata: rec.metadata.unwrap_or(serde_json::json!({})),
                expires_at: rec.expires_at,
                last_used_at: rec.last_used_at,
                is_active: rec.is_active.unwrap_or(true),
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                revoked_at: rec.revoked_at,
                revoked_reason: rec.revoked_reason,
            }).collect()
        } else {
            let recs = sqlx::query!(
                r#"SELECT id, app_id, deployment_id, name, key_prefix, key_suffix, key_hash,
                   permissions as "permissions: serde_json::Value",
                   metadata as "metadata: serde_json::Value",
                   expires_at, last_used_at, is_active, created_at, updated_at,
                   revoked_at, revoked_reason
                   FROM api_keys WHERE app_id = $1 AND deployment_id = $2 AND is_active = true ORDER BY created_at DESC"#,
                self.app_id,
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await?;
            
            recs.into_iter().map(|rec| ApiKey {
                id: rec.id,
                app_id: rec.app_id,
                deployment_id: rec.deployment_id,
                name: rec.name,
                key_prefix: rec.key_prefix,
                key_suffix: rec.key_suffix,
                key_hash: rec.key_hash,
                permissions: serde_json::from_value(rec.permissions.clone().unwrap_or(serde_json::json!([]))).unwrap_or_default(),
                metadata: rec.metadata.unwrap_or(serde_json::json!({})),
                expires_at: rec.expires_at,
                last_used_at: rec.last_used_at,
                is_active: rec.is_active.unwrap_or(true),
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                revoked_at: rec.revoked_at,
                revoked_reason: rec.revoked_reason,
            }).collect()
        };

        Ok(keys)
    }
}

pub struct GetApiKeyByHashQuery {
    pub key_hash: String,
}

impl GetApiKeyByHashQuery {
    pub fn new(key_hash: String) -> Self {
        Self { key_hash }
    }
}

impl Query for GetApiKeyByHashQuery {
    type Output = Option<ApiKey>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            r#"SELECT id, app_id, deployment_id, name, key_prefix, key_suffix, key_hash,
               permissions as "permissions: serde_json::Value",
               metadata as "metadata: serde_json::Value",
               expires_at, last_used_at, is_active, created_at, updated_at,
               revoked_at, revoked_reason
               FROM api_keys WHERE key_hash = $1 AND is_active = true"#,
            self.key_hash
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(rec.map(|rec| ApiKey {
            id: rec.id,
            app_id: rec.app_id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            key_prefix: rec.key_prefix,
            key_suffix: rec.key_suffix,
            key_hash: rec.key_hash,
            permissions: serde_json::from_value(rec.permissions.clone().unwrap_or(serde_json::json!([]))).unwrap_or_default(),
            metadata: rec.metadata.unwrap_or(serde_json::json!({})),
            expires_at: rec.expires_at,
            last_used_at: rec.last_used_at,
            is_active: rec.is_active.unwrap_or(true),
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            revoked_at: rec.revoked_at,
            revoked_reason: rec.revoked_reason,
        }))
    }
}

pub struct GetApiKeyByIdQuery {
    pub key_id: i64,
    pub deployment_id: i64,
}

impl Query for GetApiKeyByIdQuery {
    type Output = Option<ApiKey>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            r#"SELECT id, app_id, deployment_id, name, key_prefix, key_suffix, key_hash,
               permissions as "permissions: serde_json::Value",
               metadata as "metadata: serde_json::Value",
               expires_at, last_used_at, is_active, created_at, updated_at,
               revoked_at, revoked_reason
               FROM api_keys WHERE id = $1 AND deployment_id = $2"#,
            self.key_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(rec.map(|rec| ApiKey {
            id: rec.id,
            app_id: rec.app_id,
            deployment_id: rec.deployment_id,
            name: rec.name,
            key_prefix: rec.key_prefix,
            key_suffix: rec.key_suffix,
            key_hash: rec.key_hash,
            permissions: serde_json::from_value(rec.permissions.clone().unwrap_or(serde_json::json!([]))).unwrap_or_default(),
            metadata: rec.metadata.unwrap_or(serde_json::json!({})),
            expires_at: rec.expires_at,
            last_used_at: rec.last_used_at,
            is_active: rec.is_active.unwrap_or(true),
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            revoked_at: rec.revoked_at,
            revoked_reason: rec.revoked_reason,
        }))
    }
}

// This is the query needed for the backend_deployment_middleware
pub struct GetDeploymentByApiKeyQuery {
    pub api_key_hash: String,
}

impl GetDeploymentByApiKeyQuery {
    pub fn new(api_key_hash: String) -> Self {
        Self { api_key_hash }
    }
}

impl Query for GetDeploymentByApiKeyQuery {
    type Output = Option<i64>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            "SELECT deployment_id FROM api_keys WHERE key_hash = $1 AND is_active = true",
            self.api_key_hash
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(result.map(|r| r.deployment_id))
    }
}