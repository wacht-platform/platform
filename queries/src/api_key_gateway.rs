use super::Query;
use common::error::AppError;
use common::state::AppState;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Optimized query for the gateway - gets all needed data in a single query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyGatewayData {
    // API Key fields
    pub key_id: i64,
    pub deployment_id: i64,
    pub app_id: i64,
    pub key_name: String,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub permissions: Vec<String>,
    
    // API Key App fields
    pub app_name: String,
    pub rate_limit_per_minute: Option<i32>,
    pub rate_limit_per_hour: Option<i32>,
    pub rate_limit_per_day: Option<i32>,
    pub rate_limit_mode: Option<String>,
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
                a.name as app_name,
                a.rate_limit_per_minute,
                a.rate_limit_per_hour,
                a.rate_limit_per_day,
                a.rate_limit_mode
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
            permissions: serde_json::from_value(
                r.permissions.unwrap_or(serde_json::json!([]))
            ).unwrap_or_default(),
            app_name: r.app_name,
            rate_limit_per_minute: r.rate_limit_per_minute,
            rate_limit_per_hour: r.rate_limit_per_hour,
            rate_limit_per_day: r.rate_limit_per_day,
            rate_limit_mode: r.rate_limit_mode,
        }))
    }
}