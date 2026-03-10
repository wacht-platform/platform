use chrono::{DateTime, Utc};
use common::error::AppError;
use common::json_utils::json_default;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyGatewayData {
    pub key_id: i64,
    pub deployment_id: i64,
    pub app_slug: String,
    pub key_name: String,
    pub owner_user_id: Option<i64>,
    pub is_active: bool,
    pub expires_at: Option<DateTime<Utc>>,
    pub permissions: Vec<String>,
    pub org_role_permissions: Vec<String>,
    pub workspace_role_permissions: Vec<String>,
    pub metadata: serde_json::Value,
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ApiKeyGatewayData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rec = sqlx::query!(
            r#"
            SELECT
                k.id as key_id,
                k.deployment_id,
                k.app_slug,
                k.name as key_name,
                k.owner_user_id,
                k.is_active,
                k.expires_at,
                k.permissions as "permissions: serde_json::Value",
                k.org_role_permissions as "org_role_permissions: serde_json::Value",
                k.workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
                k.metadata as "metadata: serde_json::Value",
                COALESCE(k.rate_limit_scheme_slug, a.rate_limit_scheme_slug) as "rate_limit_scheme_slug?",
                k.organization_id,
                k.workspace_id,
                k.organization_membership_id,
                k.workspace_membership_id
            FROM api_keys k
            INNER JOIN api_auth_apps a
                ON a.deployment_id = k.deployment_id
               AND a.app_slug = k.app_slug
               AND a.deleted_at IS NULL
               AND a.is_active = true
            WHERE k.key_hash = $1
                AND k.is_active = true
            "#,
            self.key_hash
        )
        .fetch_optional(executor)
        .await?;

        Ok(rec.map(|r| ApiKeyGatewayData {
            key_id: r.key_id,
            deployment_id: r.deployment_id,
            app_slug: r.app_slug,
            key_name: r.key_name,
            owner_user_id: r.owner_user_id,
            is_active: r.is_active.unwrap_or(true),
            expires_at: r.expires_at,
            permissions: json_default(r.permissions.clone().unwrap_or_else(|| serde_json::json!([]))),
            org_role_permissions: if r.org_role_permissions.is_null() {
                vec![]
            } else {
                json_default(r.org_role_permissions.clone())
            },
            workspace_role_permissions: if r.workspace_role_permissions.is_null() {
                vec![]
            } else {
                json_default(r.workspace_role_permissions.clone())
            },
            metadata: r.metadata.clone().unwrap_or_else(|| serde_json::json!({})),
            rate_limit_scheme_slug: r.rate_limit_scheme_slug,
            organization_id: r.organization_id,
            workspace_id: r.workspace_id,
            organization_membership_id: r.organization_membership_id,
            workspace_membership_id: r.workspace_membership_id,
        }))
    }
}
