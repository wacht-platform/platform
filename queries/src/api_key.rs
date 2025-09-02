use super::Query;
use common::error::AppError;
use common::state::AppState;
use models::api_key::{ApiKey, ApiKeyApp, ApiKeyWithIdentifers};

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

            recs.into_iter()
                .map(|rec| ApiKeyApp {
                    id: rec.id,
                    deployment_id: rec.deployment_id,
                    name: rec.name,
                    description: rec.description,
                    is_active: rec.is_active.unwrap_or(true),
                    rate_limit_per_minute: rec.rate_limit_per_minute,
                    rate_limit_per_hour: rec.rate_limit_per_hour,
                    rate_limit_per_day: rec.rate_limit_per_day,
                    rate_limit_mode: rec
                        .rate_limit_mode
                        .and_then(|s| models::api_key::RateLimitMode::from_str(&s)),
                    created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                    deleted_at: rec.deleted_at,
                })
                .collect()
        } else {
            let recs = sqlx::query!(
                "SELECT * FROM api_key_apps WHERE deployment_id = $1 AND is_active = true AND deleted_at IS NULL ORDER BY created_at DESC",
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await?;

            recs.into_iter()
                .map(|rec| ApiKeyApp {
                    id: rec.id,
                    deployment_id: rec.deployment_id,
                    name: rec.name,
                    description: rec.description,
                    is_active: rec.is_active.unwrap_or(true),
                    rate_limit_per_minute: rec.rate_limit_per_minute,
                    rate_limit_per_hour: rec.rate_limit_per_hour,
                    rate_limit_per_day: rec.rate_limit_per_day,
                    rate_limit_mode: rec
                        .rate_limit_mode
                        .and_then(|s| models::api_key::RateLimitMode::from_str(&s)),
                    created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                    deleted_at: rec.deleted_at,
                })
                .collect()
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
            rate_limit_per_day: rec.rate_limit_per_day,
            rate_limit_mode: rec
                .rate_limit_mode
                .and_then(|s| models::api_key::RateLimitMode::from_str(&s)),
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
            rate_limit_per_day: rec.rate_limit_per_day,
            rate_limit_mode: rec
                .rate_limit_mode
                .and_then(|s| models::api_key::RateLimitMode::from_str(&s)),
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

            recs.into_iter()
                .map(|rec| ApiKey {
                    id: rec.id,
                    app_id: rec.app_id,
                    deployment_id: rec.deployment_id,
                    name: rec.name,
                    key_prefix: rec.key_prefix,
                    key_suffix: rec.key_suffix,
                    key_hash: rec.key_hash,
                    permissions: serde_json::from_value(
                        rec.permissions.clone().unwrap_or(serde_json::json!([])),
                    )
                    .unwrap_or_default(),
                    metadata: rec.metadata.unwrap_or(serde_json::json!({})),
                    expires_at: rec.expires_at,
                    last_used_at: rec.last_used_at,
                    is_active: rec.is_active.unwrap_or(true),
                    created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                    revoked_at: rec.revoked_at,
                    revoked_reason: rec.revoked_reason,
                })
                .collect()
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

            recs.into_iter()
                .map(|rec| ApiKey {
                    id: rec.id,
                    app_id: rec.app_id,
                    deployment_id: rec.deployment_id,
                    name: rec.name,
                    key_prefix: rec.key_prefix,
                    key_suffix: rec.key_suffix,
                    key_hash: rec.key_hash,
                    permissions: serde_json::from_value(
                        rec.permissions.clone().unwrap_or(serde_json::json!([])),
                    )
                    .unwrap_or_default(),
                    metadata: rec.metadata.unwrap_or(serde_json::json!({})),
                    expires_at: rec.expires_at,
                    last_used_at: rec.last_used_at,
                    is_active: rec.is_active.unwrap_or(true),
                    created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                    revoked_at: rec.revoked_at,
                    revoked_reason: rec.revoked_reason,
                })
                .collect()
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
            r#"SELECT k.id, k.app_id, a.name as app_name,
                   k.deployment_id, k.name, k.key_prefix,
                   k.key_suffix, k.key_hash,
                   k.permissions as "permissions: serde_json::Value",
                   k.metadata as "metadata: serde_json::Value",
                   k.expires_at, k.last_used_at, k.is_active,
                   k.created_at, k.updated_at,
                   k.revoked_at, k.revoked_reason
                FROM api_keys k
                LEFT JOIN api_key_apps a ON k.app_id = a.id
                WHERE k.key_hash = $1 AND k.is_active = true
               "#,
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
            permissions: serde_json::from_value(
                rec.permissions.clone().unwrap_or(serde_json::json!([])),
            )
            .unwrap_or_default(),
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

pub struct GetApiKeyIdentifiersByHashQuery {
    pub key_hash: String,
}

impl GetApiKeyIdentifiersByHashQuery {
    pub fn new(key_hash: String) -> Self {
        Self { key_hash }
    }
}

impl Query for GetApiKeyIdentifiersByHashQuery {
    type Output = Option<ApiKeyWithIdentifers>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            r#"SELECT k.id as id, a.name as app_name,
            k.permissions as "permissions: serde_json::Value",
            k.is_active as is_active,
            k.expires_at as expires_at
            FROM api_keys k
            LEFT JOIN api_key_apps a ON k.app_id = a.id
            WHERE k.key_hash = $1 AND k.is_active = true"#,
            self.key_hash
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(rec.map(|rec| ApiKeyWithIdentifers {
            app_name: rec.app_name,
            id: rec.id,
            permissions: serde_json::from_value(rec.permissions.unwrap_or(serde_json::json!([])))
                .unwrap_or_default(),
            expires_at: rec.expires_at,
            is_active: rec.is_active.unwrap_or(true),
        }))
    }
}
