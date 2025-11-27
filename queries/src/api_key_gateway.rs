use super::Query;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use models::api_key::RateLimit;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyGatewayData {
    pub key_id: i64,
    pub deployment_id: i64,
    pub app_id: i64,
    pub key_name: String,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub permissions: Vec<String>,
    pub metadata: serde_json::Value,
    pub app_name: String,
    pub rate_limits: Vec<RateLimit>,
}

pub struct GetApiKeyGatewayDataQuery {
    pub key_hash: String,
}

impl GetApiKeyGatewayDataQuery {
    pub fn new(key_hash: String) -> Self {
        Self { key_hash }
    }
}

impl Query for GetApiKeyGatewayDataQuery {
    type Output = Option<ApiKeyGatewayData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rec = sqlx::query!(
            r#"
            SELECT
                k.id as key_id,
                k.deployment_id,
                k.app_id,
                k.name as key_name,
                k.is_active,
                k.expires_at,
                k.permissions as "permissions: serde_json::Value",
                k.metadata as "metadata: serde_json::Value",
                a.name as app_name,
                a.rate_limits as "rate_limits: serde_json::Value"
            FROM api_keys k
            INNER JOIN api_key_apps a ON k.app_id = a.id
            WHERE k.key_hash = $1
                AND k.is_active = true
                AND a.is_active = true
                AND a.deleted_at IS NULL
            "#,
            self.key_hash
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(rec.map(|r| ApiKeyGatewayData {
            key_id: r.key_id,
            deployment_id: r.deployment_id,
            app_id: r.app_id,
            key_name: r.key_name,
            is_active: r.is_active.unwrap_or(true),
            expires_at: r.expires_at,
            permissions: serde_json::from_value(r.permissions.unwrap_or(serde_json::json!([])))
                .unwrap_or_default(),
            metadata: r.metadata.unwrap_or(serde_json::json!({})),
            app_name: r.app_name,
            rate_limits: serde_json::from_value(r.rate_limits.unwrap_or(serde_json::json!([])))
                .unwrap_or_else(|_| vec![]),
        }))
    }
}
