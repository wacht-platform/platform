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
    pub app_id: i64, // TODO: remove when switching to app_slug only
    pub app_slug: String,
    pub key_name: String,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub permissions: Vec<String>,
    pub org_role_permissions: Vec<String>,
    pub workspace_role_permissions: Vec<String>,
    pub metadata: serde_json::Value,
    pub rate_limits: Vec<RateLimit>,
    pub rate_limit_scheme_slug: Option<String>,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub organization_membership_id: Option<i64>,
    pub workspace_membership_id: Option<i64>,
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
                k.app_slug,
                k.name as key_name,
                k.is_active,
                k.expires_at,
                k.permissions as "permissions: serde_json::Value",
                k.org_role_permissions as "org_role_permissions: serde_json::Value",
                k.workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
                k.metadata as "metadata: serde_json::Value",
                k.rate_limits as "rate_limits: serde_json::Value",
                k.rate_limit_scheme_slug,
                k.organization_id,
                k.workspace_id,
                k.organization_membership_id,
                k.workspace_membership_id
            FROM api_keys k
            WHERE k.key_hash = $1
                AND k.is_active = true
            "#,
            self.key_hash
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(rec.map(|r| ApiKeyGatewayData {
            key_id: r.key_id,
            deployment_id: r.deployment_id,
            app_id: r.app_id,
            app_slug: r.app_slug,
            key_name: r.key_name,
            is_active: r.is_active.unwrap_or(true),
            expires_at: r.expires_at,
            permissions: serde_json::from_value(
                r.permissions
                    .clone()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default(),
            org_role_permissions: if r.org_role_permissions.is_null() {
                vec![]
            } else {
                serde_json::from_value(r.org_role_permissions.clone()).unwrap_or_default()
            },
            workspace_role_permissions: if r.workspace_role_permissions.is_null() {
                vec![]
            } else {
                serde_json::from_value(r.workspace_role_permissions.clone()).unwrap_or_default()
            },
            metadata: r.metadata.clone().unwrap_or_else(|| serde_json::json!({})),
            rate_limits: if r.rate_limits.is_null() {
                vec![]
            } else {
                serde_json::from_value(r.rate_limits.clone()).unwrap_or_else(|_| vec![])
            },
            rate_limit_scheme_slug: r.rate_limit_scheme_slug,
            organization_id: r.organization_id,
            workspace_id: r.workspace_id,
            organization_membership_id: r.organization_membership_id,
            workspace_membership_id: r.workspace_membership_id,
        }))
    }
}
