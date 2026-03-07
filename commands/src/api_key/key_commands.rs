use chrono::{DateTime, Utc};
use common::error::AppError;
use models::api_key::{ApiKey, ApiKeyWithSecret};
use sha2::{Digest, Sha256};

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
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
    ) -> Result<ApiKeyWithSecret, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<ApiKeyWithSecret, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
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
        .fetch_one(&mut *conn)
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
            owner_user_id: rec.owner_user_id,
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

impl RevokeApiKeyCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<(), AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
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
        .execute(&mut *conn)
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
    pub new_key_id: Option<i64>,
}

impl RotateApiKeyCommand {
    pub fn with_new_key_id(mut self, new_key_id: i64) -> Self {
        self.new_key_id = Some(new_key_id);
        self
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
    ) -> Result<ApiKeyWithSecret, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<ApiKeyWithSecret, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let new_key_id = self
            .new_key_id
            .ok_or_else(|| AppError::Validation("new_key_id is required".to_string()))?;
        // Get the existing key
        let rec = sqlx::query!(
            r#"
            SELECT id, deployment_id, app_slug, name, key_prefix, key_suffix,
                   permissions as "permissions: serde_json::Value",
                   metadata as "metadata: serde_json::Value",
                   rate_limit_scheme_slug,
                   owner_user_id,
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
        .fetch_optional(&mut *conn)
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
            owner_user_id: rec.owner_user_id,
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

        let app_context = sqlx::query!(
            r#"
            SELECT user_id, organization_id, workspace_id
            FROM api_auth_apps
            WHERE deployment_id = $1 AND app_slug = $2 AND deleted_at IS NULL AND is_active = true
            "#,
            self.deployment_id,
            existing_key.app_slug.clone()
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("API key app not found or inactive".to_string()))?;

        if app_context.user_id.is_none()
            && (app_context.organization_id.is_some() || app_context.workspace_id.is_some())
        {
            return Err(AppError::BadRequest(
                "user is not a member of the org".to_string(),
            ));
        }

        let mut org_membership_id: Option<i64> = None;
        let mut workspace_membership_id: Option<i64> = None;

        if let (Some(user_id), Some(organization_id)) =
            (app_context.user_id, app_context.organization_id)
        {
            let org_membership = sqlx::query!(
                r#"
                SELECT id
                FROM organization_memberships
                WHERE user_id = $1
                  AND organization_id = $2
                  AND deleted_at IS NULL
                LIMIT 1
                "#,
                user_id,
                organization_id
            )
            .fetch_optional(&mut *conn)
            .await?;

            org_membership_id = org_membership.map(|r| r.id);
            if org_membership_id.is_none() {
                return Err(AppError::BadRequest(
                    "user is not a member of the org".to_string(),
                ));
            }
        }

        if let (Some(user_id), Some(workspace_id)) = (app_context.user_id, app_context.workspace_id)
        {
            let workspace_membership = sqlx::query!(
                r#"
                SELECT id
                FROM workspace_memberships
                WHERE user_id = $1
                  AND workspace_id = $2
                  AND deleted_at IS NULL
                LIMIT 1
                "#,
                user_id,
                workspace_id
            )
            .fetch_optional(&mut *conn)
            .await?;

            workspace_membership_id = workspace_membership.map(|r| r.id);
            if workspace_membership_id.is_none() {
                return Err(AppError::BadRequest(
                    "user is not a member of the org".to_string(),
                ));
            }
        }

        let mut organization_id: Option<i64> = None;
        let mut workspace_id: Option<i64> = None;
        let mut org_role_permissions: Vec<String> = vec![];
        let mut workspace_role_permissions: Vec<String> = vec![];

        if let Some(org_membership_id) = org_membership_id {
            let org_perm = sqlx::query!(
                r#"
                SELECT
                    om.organization_id,
                    COALESCE(
                        jsonb_agg(DISTINCT perm) FILTER (WHERE perm IS NOT NULL),
                        '[]'::jsonb
                    ) as "permissions: serde_json::Value"
                FROM organization_memberships om
                LEFT JOIN organization_membership_roles omr ON omr.organization_membership_id = om.id
                LEFT JOIN organization_roles orole ON omr.organization_role_id = orole.id
                LEFT JOIN LATERAL unnest(COALESCE(orole.permissions, ARRAY[]::text[])) perm ON true
                WHERE om.id = $1 AND om.deleted_at IS NULL
                GROUP BY om.organization_id
                "#,
                org_membership_id
            )
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| AppError::NotFound("Organization membership not found".to_string()))?;

            organization_id = Some(org_perm.organization_id);
            org_role_permissions = serde_json::from_value(
                org_perm
                    .permissions
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default();
        }

        if let Some(workspace_membership_id) = workspace_membership_id {
            let workspace_perm = sqlx::query!(
                r#"
                SELECT
                    wm.organization_id,
                    wm.workspace_id,
                    COALESCE(
                        jsonb_agg(DISTINCT perm) FILTER (WHERE perm IS NOT NULL),
                        '[]'::jsonb
                    ) as "permissions: serde_json::Value"
                FROM workspace_memberships wm
                LEFT JOIN workspace_membership_roles wmr ON wmr.workspace_membership_id = wm.id
                LEFT JOIN workspace_roles wrole ON wmr.workspace_role_id = wrole.id
                LEFT JOIN LATERAL unnest(COALESCE(wrole.permissions, ARRAY[]::text[])) perm ON true
                WHERE wm.id = $1 AND wm.deleted_at IS NULL
                GROUP BY wm.organization_id, wm.workspace_id
                "#,
                workspace_membership_id
            )
            .fetch_optional(&mut *conn)
            .await?
            .ok_or_else(|| AppError::NotFound("Workspace membership not found".to_string()))?;

            if let Some(existing_org_id) = organization_id {
                if existing_org_id != workspace_perm.organization_id {
                    return Err(AppError::BadRequest(
                        "organization_membership_id and workspace_membership_id belong to different organizations"
                            .to_string(),
                    ));
                }
            }

            organization_id = Some(workspace_perm.organization_id);
            workspace_id = Some(workspace_perm.workspace_id);
            workspace_role_permissions = serde_json::from_value(
                workspace_perm
                    .permissions
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default();
        }

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
        .execute(&mut *conn)
        .await?;

        // Create a new key with the same settings
        let create_command = CreateApiKeyCommand {
            key_id: Some(new_key_id),
            app_slug: existing_key.app_slug,
            deployment_id: existing_key.deployment_id,
            name: existing_key.name,
            key_prefix: existing_key.key_prefix,
            permissions: existing_key.permissions,
            metadata: Some(existing_key.metadata),
            expires_at: existing_key.expires_at,
            rate_limit_scheme_slug: existing_key.rate_limit_scheme_slug,
            owner_user_id: app_context.user_id,
            organization_id,
            workspace_id,
            organization_membership_id: org_membership_id,
            workspace_membership_id,
            org_role_permissions,
            workspace_role_permissions,
        };

        create_command.execute_with_deps(&mut *conn).await
    }
}
