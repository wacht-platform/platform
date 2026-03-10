use chrono::{DateTime, Utc};
use common::error::AppError;
use models::api_key::ApiKeyWithSecret;
use sha2::{Digest, Sha256};

use super::shared::build_api_key_model;

pub struct CreateApiKeyCommand {
    pub key_id: Option<i64>,
    pub app_slug: String,
    pub deployment_id: i64,
    pub name: String,
    pub key_prefix: String, // 'sk_live_', 'sk_test_', etc.
    pub permissions: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub expires_at: Option<DateTime<Utc>>,
    pub rate_limit_scheme_slug: Option<String>,
    pub owner_user_id: Option<i64>,
    pub organization_id: Option<i64>,
    pub workspace_id: Option<i64>,
    pub organization_membership_id: Option<i64>,
    pub workspace_membership_id: Option<i64>,
    pub org_role_permissions: Vec<String>,
    pub workspace_role_permissions: Vec<String>,
}

impl CreateApiKeyCommand {
    pub fn new(app_slug: String, deployment_id: i64, name: String, key_prefix: String) -> Self {
        Self {
            key_id: None,
            app_slug,
            deployment_id,
            name,
            key_prefix,
            permissions: vec![],
            metadata: None,
            expires_at: None,
            rate_limit_scheme_slug: None,
            owner_user_id: None,
            organization_id: None,
            workspace_id: None,
            organization_membership_id: None,
            workspace_membership_id: None,
            org_role_permissions: vec![],
            workspace_role_permissions: vec![],
        }
    }

    pub fn with_permissions(mut self, permissions: Vec<String>) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn with_expiration(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    pub fn with_rate_limit_scheme_slug(mut self, slug: Option<String>) -> Self {
        self.rate_limit_scheme_slug = slug;
        self
    }

    pub fn with_membership_context(
        mut self,
        organization_id: Option<i64>,
        workspace_id: Option<i64>,
        organization_membership_id: Option<i64>,
        workspace_membership_id: Option<i64>,
        org_role_permissions: Vec<String>,
        workspace_role_permissions: Vec<String>,
    ) -> Self {
        self.organization_id = organization_id;
        self.workspace_id = workspace_id;
        self.organization_membership_id = organization_membership_id;
        self.workspace_membership_id = workspace_membership_id;
        self.org_role_permissions = org_role_permissions;
        self.workspace_role_permissions = workspace_role_permissions;
        self
    }

    pub fn with_key_id(mut self, key_id: i64) -> Self {
        self.key_id = Some(key_id);
        self
    }

    fn generate_api_key(&self) -> (String, String, String) {
        // Generate 32 random bytes
        use rand::RngCore;
        let mut random_bytes = vec![0u8; 32];
        rand::rng().fill_bytes(&mut random_bytes);

        // Encode to URL-safe base64
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let key_string = URL_SAFE_NO_PAD.encode(&random_bytes);
        let full_key = format!("{}_{}", self.key_prefix, key_string);

        // Hash the key
        let mut hasher = Sha256::new();
        hasher.update(full_key.as_bytes());
        let key_hash = format!("{:x}", hasher.finalize());

        // Get last 8 characters as suffix
        let key_suffix = full_key
            .chars()
            .rev()
            .take(8)
            .collect::<String>()
            .chars()
            .rev()
            .collect();

        (full_key, key_hash, key_suffix)
    }
}

impl CreateApiKeyCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ApiKeyWithSecret, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let key_id = self
            .key_id
            .ok_or_else(|| AppError::Validation("key_id is required".to_string()))?;
        let (full_key, key_hash, key_suffix) = self.generate_api_key();

        let rec = sqlx::query!(
            r#"
            INSERT INTO api_keys (
                id, deployment_id, app_slug, name, key_prefix, key_hash, key_suffix,
                permissions, metadata, rate_limit_scheme_slug, expires_at,
                owner_user_id,
                organization_id, workspace_id, organization_membership_id, workspace_membership_id,
                org_role_permissions, workspace_role_permissions
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            RETURNING id, deployment_id, app_slug, name, key_prefix, key_suffix, key_hash,
                      permissions as "permissions: serde_json::Value",
                      metadata as "metadata: serde_json::Value",
                      rate_limit_scheme_slug,
                      owner_user_id,
                      organization_id, workspace_id, organization_membership_id, workspace_membership_id,
                      org_role_permissions as "org_role_permissions: serde_json::Value",
                      workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
                      expires_at, last_used_at, is_active, created_at, updated_at,
                      revoked_at, revoked_reason
            "#,
            key_id,
            self.deployment_id,
            self.app_slug,
            self.name,
            self.key_prefix,
            key_hash,
            key_suffix,
            serde_json::to_value(&self.permissions)?,
            self.metadata.unwrap_or(serde_json::json!({})),
            self.rate_limit_scheme_slug,
            self.expires_at,
            self.owner_user_id,
            self.organization_id,
            self.workspace_id,
            self.organization_membership_id,
            self.workspace_membership_id,
            serde_json::to_value(&self.org_role_permissions)?,
            serde_json::to_value(&self.workspace_role_permissions)?,
        )
        .fetch_one(executor)
        .await?;

        let key = build_api_key_model(
            rec.id,
            rec.deployment_id,
            rec.app_slug,
            rec.name,
            rec.key_prefix,
            rec.key_suffix,
            rec.key_hash,
            rec.permissions.clone(),
            rec.metadata.clone(),
            rec.rate_limit_scheme_slug,
            rec.owner_user_id,
            rec.organization_id,
            rec.workspace_id,
            rec.organization_membership_id,
            rec.workspace_membership_id,
            rec.org_role_permissions.clone(),
            rec.workspace_role_permissions.clone(),
            rec.expires_at,
            rec.last_used_at,
            rec.is_active,
            rec.created_at,
            rec.updated_at,
            rec.revoked_at,
            rec.revoked_reason,
        );

        Ok(ApiKeyWithSecret {
            key,
            secret: full_key,
        })
    }
}

