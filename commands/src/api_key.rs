use crate::Command;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use models::api_key::{ApiKey, ApiKeyWithSecret};
use sha2::{Digest, Sha256};

pub struct CreateApiKeyCommand {
    pub app_slug: String,
    pub deployment_id: i64,
    pub name: String,
    pub key_prefix: String, // 'sk_live_', 'sk_test_', etc.
    pub permissions: Vec<String>,
    pub metadata: Option<serde_json::Value>,
    pub expires_at: Option<DateTime<Utc>>,
    pub rate_limit_scheme_slug: Option<String>,
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
            app_slug,
            deployment_id,
            name,
            key_prefix,
            permissions: vec![],
            metadata: None,
            expires_at: None,
            rate_limit_scheme_slug: None,
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

impl Command for CreateApiKeyCommand {
    type Output = ApiKeyWithSecret;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let (full_key, key_hash, key_suffix) = self.generate_api_key();

        // Generate Snowflake ID
        let key_id = app_state.sf.next_id()? as i64;

        let rec = sqlx::query!(
            r#"
            INSERT INTO api_keys (
                id, deployment_id, app_slug, name, key_prefix, key_hash, key_suffix,
                permissions, metadata, rate_limit_scheme_slug, expires_at,
                organization_id, workspace_id, organization_membership_id, workspace_membership_id,
                org_role_permissions, workspace_role_permissions
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            RETURNING id, deployment_id, app_slug, name, key_prefix, key_suffix, key_hash,
                      permissions as "permissions: serde_json::Value",
                      metadata as "metadata: serde_json::Value",
                      rate_limit_scheme_slug,
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
            self.organization_id,
            self.workspace_id,
            self.organization_membership_id,
            self.workspace_membership_id,
            serde_json::to_value(&self.org_role_permissions)?,
            serde_json::to_value(&self.workspace_role_permissions)?,
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        let key = ApiKey {
            id: rec.id,
            deployment_id: rec.deployment_id,
            app_slug: rec.app_slug,
            name: rec.name,
            key_prefix: rec.key_prefix,
            key_suffix: rec.key_suffix,
            key_hash: rec.key_hash,
            permissions: serde_json::from_value(
                rec.permissions
                    .clone()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default(),
            metadata: rec
                .metadata
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
            rate_limits: vec![],
            rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
            organization_id: rec.organization_id,
            workspace_id: rec.workspace_id,
            organization_membership_id: rec.organization_membership_id,
            workspace_membership_id: rec.workspace_membership_id,
            org_role_permissions: if rec.org_role_permissions.is_null() {
                vec![]
            } else {
                serde_json::from_value(rec.org_role_permissions.clone()).unwrap_or_default()
            },
            workspace_role_permissions: if rec.workspace_role_permissions.is_null() {
                vec![]
            } else {
                serde_json::from_value(rec.workspace_role_permissions.clone()).unwrap_or_default()
            },
            expires_at: rec.expires_at,
            last_used_at: rec.last_used_at,
            is_active: rec.is_active.unwrap_or(true),
            created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
            updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
            revoked_at: rec.revoked_at,
            revoked_reason: rec.revoked_reason,
        };

        Ok(ApiKeyWithSecret {
            key,
            secret: full_key,
        })
    }
}

pub struct RevokeApiKeyCommand {
    pub key_id: i64,
    pub deployment_id: i64,
    pub reason: Option<String>,
}

impl Command for RevokeApiKeyCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            UPDATE api_keys
            SET
                is_active = false,
                revoked_at = NOW(),
                revoked_reason = $3,
                updated_at = NOW()
            WHERE id = $1 AND deployment_id = $2 AND is_active = true
            "#,
            self.key_id,
            self.deployment_id,
            self.reason
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(
                "API key not found or already revoked".to_string(),
            ));
        }

        Ok(())
    }
}

pub struct RotateApiKeyCommand {
    pub key_id: i64,
    pub deployment_id: i64,
}

impl Command for RotateApiKeyCommand {
    type Output = ApiKeyWithSecret;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get the existing key
        let rec = sqlx::query!(
            r#"
            SELECT id, deployment_id, app_slug, name, key_prefix, key_suffix,
                   permissions as "permissions: serde_json::Value",
                   metadata as "metadata: serde_json::Value",
                   rate_limit_scheme_slug,
                   organization_id, workspace_id, organization_membership_id, workspace_membership_id,
                   org_role_permissions as "org_role_permissions: serde_json::Value",
                   workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
                   expires_at
            FROM api_keys
            WHERE id = $1 AND deployment_id = $2 AND is_active = true
            "#,
            self.key_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?
        .ok_or_else(|| AppError::NotFound("API key not found or inactive".to_string()))?;

        let existing_key = ApiKey {
            id: rec.id,
            deployment_id: rec.deployment_id,
            app_slug: rec.app_slug,
            name: rec.name,
            key_prefix: rec.key_prefix,
            key_suffix: rec.key_suffix,
            key_hash: String::new(), // Not needed for rotation
            permissions: serde_json::from_value(
                rec.permissions
                    .clone()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default(),
            metadata: rec
                .metadata
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
            rate_limits: vec![],
            rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
            organization_id: rec.organization_id,
            workspace_id: rec.workspace_id,
            organization_membership_id: rec.organization_membership_id,
            workspace_membership_id: rec.workspace_membership_id,
            org_role_permissions: if rec.org_role_permissions.is_null() {
                vec![]
            } else {
                serde_json::from_value(rec.org_role_permissions.clone()).unwrap_or_default()
            },
            workspace_role_permissions: if rec.workspace_role_permissions.is_null() {
                vec![]
            } else {
                serde_json::from_value(rec.workspace_role_permissions.clone()).unwrap_or_default()
            },
            expires_at: rec.expires_at,
            last_used_at: None,
            is_active: true,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            revoked_at: None,
            revoked_reason: None,
        };

        // Revoke the old key
        sqlx::query!(
            r#"
            UPDATE api_keys
            SET
                is_active = false,
                revoked_at = NOW(),
                revoked_reason = 'Rotated',
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.key_id
        )
        .execute(&app_state.db_pool)
        .await?;

        // Create a new key with the same settings
        let create_command = CreateApiKeyCommand {
            app_slug: existing_key.app_slug,
            deployment_id: existing_key.deployment_id,
            name: existing_key.name,
            key_prefix: existing_key.key_prefix,
            permissions: existing_key.permissions,
            metadata: Some(existing_key.metadata),
            expires_at: existing_key.expires_at,
            rate_limit_scheme_slug: existing_key.rate_limit_scheme_slug,
            organization_id: existing_key.organization_id,
            workspace_id: existing_key.workspace_id,
            organization_membership_id: existing_key.organization_membership_id,
            workspace_membership_id: existing_key.workspace_membership_id,
            org_role_permissions: existing_key.org_role_permissions,
            workspace_role_permissions: existing_key.workspace_role_permissions,
        };

        create_command.execute(app_state).await
    }
}

pub struct UpdateApiKeyLastUsedCommand {
    pub key_id: i64,
}

impl Command for UpdateApiKeyLastUsedCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            r#"
            UPDATE api_keys
            SET last_used_at = NOW()
            WHERE id = $1
            "#,
            self.key_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}
