use super::rate_limit_scheme::GetRateLimitSchemeQuery;
use common::error::AppError;
use models::api_key::{ApiAuthApp, ApiKey, ApiKeyWithIdentifers, RateLimit};

async fn resolve_rate_limits_on_conn(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    scheme_slug: &Option<String>,
) -> Result<Vec<RateLimit>, AppError> {
    let slug = match scheme_slug {
        Some(slug) => slug,
        None => return Ok(vec![]),
    };

    let scheme = GetRateLimitSchemeQuery::new(deployment_id, slug.clone())
        .execute_with_deps(conn)
        .await?;

    Ok(scheme.map(|s| s.rules).unwrap_or_default())
}

pub struct GetApiAuthAppsQuery {
    pub deployment_id: i64,
    pub include_inactive: bool,
}

impl GetApiAuthAppsQuery {
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

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<ApiAuthApp>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let recs = sqlx::query!(
            r#"SELECT deployment_id, user_id, organization_id, workspace_id, app_slug, name, key_prefix, description, is_active,
               rate_limit_scheme_slug, permissions as "permissions: serde_json::Value", resources as "resources: serde_json::Value",
               created_at, updated_at, deleted_at
               FROM api_auth_apps
               WHERE deployment_id = $1
                 AND deleted_at IS NULL
                 AND ($2 OR is_active = true)
               ORDER BY created_at DESC"#,
            self.deployment_id,
            self.include_inactive
        )
        .fetch_all(&mut *conn)
        .await?;

        let mut apps = Vec::with_capacity(recs.len());
        for rec in recs {
            let rate_limits = resolve_rate_limits_on_conn(
                &mut conn,
                rec.deployment_id,
                &rec.rate_limit_scheme_slug,
            )
            .await?;
            apps.push(ApiAuthApp {
                deployment_id: rec.deployment_id,
                user_id: rec.user_id,
                organization_id: rec.organization_id,
                workspace_id: rec.workspace_id,
                app_slug: rec.app_slug,
                name: rec.name,
                description: rec.description,
                is_active: rec.is_active.unwrap_or(true),
                key_prefix: rec.key_prefix,
                permissions: serde_json::from_value(rec.permissions.clone()).unwrap_or_default(),
                resources: serde_json::from_value(rec.resources.clone()).unwrap_or_default(),
                rate_limits,
                rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                deleted_at: rec.deleted_at,
            });
        }

        Ok(apps)
    }
}

pub struct GetApiAuthAppBySlugQuery {
    pub deployment_id: i64,
    pub app_slug: String,
}

impl GetApiAuthAppBySlugQuery {
    pub fn new(deployment_id: i64, app_slug: String) -> Self {
        Self {
            deployment_id,
            app_slug,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<ApiAuthApp>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
            r#"SELECT deployment_id, user_id, organization_id, workspace_id, app_slug, name, key_prefix, description, is_active,
               rate_limit_scheme_slug, permissions as "permissions: serde_json::Value", resources as "resources: serde_json::Value",
               created_at, updated_at, deleted_at
               FROM api_auth_apps WHERE deployment_id = $1 AND app_slug = $2 AND deleted_at IS NULL"#,
            self.deployment_id,
            self.app_slug
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(rec) = rec {
            let rate_limits = resolve_rate_limits_on_conn(
                &mut conn,
                rec.deployment_id,
                &rec.rate_limit_scheme_slug,
            )
            .await?;
            Ok(Some(ApiAuthApp {
                deployment_id: rec.deployment_id,
                user_id: rec.user_id,
                organization_id: rec.organization_id,
                workspace_id: rec.workspace_id,
                app_slug: rec.app_slug,
                name: rec.name,
                description: rec.description,
                is_active: rec.is_active.unwrap_or(true),
                key_prefix: rec.key_prefix,
                permissions: serde_json::from_value(rec.permissions.clone()).unwrap_or_default(),
                resources: serde_json::from_value(rec.resources.clone()).unwrap_or_default(),
                rate_limits,
                rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                deleted_at: rec.deleted_at,
            }))
        } else {
            Ok(None)
        }
    }
}

pub struct GetApiAuthAppByNameQuery {
    pub deployment_id: i64,
    pub name: String,
}

impl GetApiAuthAppByNameQuery {
    pub fn new(deployment_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            name,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<ApiAuthApp>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
            r#"SELECT deployment_id, user_id, organization_id, workspace_id, app_slug, name, key_prefix, description, is_active,
               rate_limit_scheme_slug, permissions as "permissions: serde_json::Value", resources as "resources: serde_json::Value",
               created_at, updated_at, deleted_at
               FROM api_auth_apps WHERE deployment_id = $1 AND name = $2 AND deleted_at IS NULL"#,
            self.deployment_id,
            self.name
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(rec) = rec {
            let rate_limits = resolve_rate_limits_on_conn(
                &mut conn,
                rec.deployment_id,
                &rec.rate_limit_scheme_slug,
            )
            .await?;
            Ok(Some(ApiAuthApp {
                deployment_id: rec.deployment_id,
                user_id: rec.user_id,
                organization_id: rec.organization_id,
                workspace_id: rec.workspace_id,
                app_slug: rec.app_slug,
                name: rec.name,
                description: rec.description,
                is_active: rec.is_active.unwrap_or(true),
                key_prefix: rec.key_prefix,
                permissions: serde_json::from_value(rec.permissions.clone()).unwrap_or_default(),
                resources: serde_json::from_value(rec.resources.clone()).unwrap_or_default(),
                rate_limits,
                rate_limit_scheme_slug: rec.rate_limit_scheme_slug,
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                deleted_at: rec.deleted_at,
            }))
        } else {
            Ok(None)
        }
    }
}

pub struct GetApiKeysByAppQuery {
    pub app_slug: String,
    pub deployment_id: i64,
    pub include_inactive: bool,
}

impl GetApiKeysByAppQuery {
    pub fn new(app_slug: String, deployment_id: i64) -> Self {
        Self {
            app_slug,
            deployment_id,
            include_inactive: false,
        }
    }

    pub fn with_inactive(mut self, include: bool) -> Self {
        self.include_inactive = include;
        self
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<ApiKey>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        if self.include_inactive {
            let recs = sqlx::query!(
                r#"SELECT id, deployment_id, app_slug, name, key_prefix, key_suffix, key_hash,
                   permissions as "permissions: serde_json::Value",
                   metadata as "metadata: serde_json::Value",
                   rate_limit_scheme_slug,
                   owner_user_id,
                   organization_id, workspace_id, organization_membership_id, workspace_membership_id,
                   org_role_permissions as "org_role_permissions: serde_json::Value",
                   workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
                   expires_at, last_used_at, is_active, created_at, updated_at,
                   revoked_at, revoked_reason
                   FROM api_keys WHERE app_slug = $1 AND deployment_id = $2 ORDER BY created_at DESC"#,
                self.app_slug,
                self.deployment_id
            )
            .fetch_all(&mut *conn)
            .await?;

            let mut keys = Vec::with_capacity(recs.len());
            for rec in recs {
                let rate_limits = resolve_rate_limits_on_conn(
                    &mut conn,
                    rec.deployment_id,
                    &rec.rate_limit_scheme_slug,
                )
                .await?;
                keys.push(ApiKey {
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
                    rate_limits,
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
                        serde_json::from_value(rec.workspace_role_permissions.clone())
                            .unwrap_or_default()
                    },
                    expires_at: rec.expires_at,
                    last_used_at: rec.last_used_at,
                    is_active: rec.is_active.unwrap_or(true),
                    created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                    updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                    revoked_at: rec.revoked_at,
                    revoked_reason: rec.revoked_reason,
                });
            }

            return Ok(keys);
        }

        let recs = sqlx::query!(
            r#"SELECT id, deployment_id, app_slug, name, key_prefix, key_suffix, key_hash,
               permissions as "permissions: serde_json::Value",
               metadata as "metadata: serde_json::Value",
               rate_limit_scheme_slug,
               owner_user_id,
               organization_id, workspace_id, organization_membership_id, workspace_membership_id,
               org_role_permissions as "org_role_permissions: serde_json::Value",
               workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
               expires_at, last_used_at, is_active, created_at, updated_at,
               revoked_at, revoked_reason
               FROM api_keys WHERE app_slug = $1 AND deployment_id = $2 AND is_active = true ORDER BY created_at DESC"#,
            self.app_slug,
            self.deployment_id
        )
        .fetch_all(&mut *conn)
        .await?;

        let mut keys = Vec::with_capacity(recs.len());
        for rec in recs {
            let rate_limits = resolve_rate_limits_on_conn(
                &mut conn,
                rec.deployment_id,
                &rec.rate_limit_scheme_slug,
            )
            .await?;
            keys.push(ApiKey {
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
                rate_limits,
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
                    serde_json::from_value(rec.workspace_role_permissions.clone())
                        .unwrap_or_default()
                },
                expires_at: rec.expires_at,
                last_used_at: rec.last_used_at,
                is_active: rec.is_active.unwrap_or(true),
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                revoked_at: rec.revoked_at,
                revoked_reason: rec.revoked_reason,
            });
        }

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

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<ApiKey>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
            r#"SELECT k.id, k.app_slug,
                   k.deployment_id, k.name, k.key_prefix,
                   k.key_suffix, k.key_hash,
                   k.permissions as "permissions: serde_json::Value",
                   k.metadata as "metadata: serde_json::Value",
                   k.rate_limit_scheme_slug,
                   k.owner_user_id,
                   k.organization_id, k.workspace_id, k.organization_membership_id, k.workspace_membership_id,
                   k.org_role_permissions as "org_role_permissions: serde_json::Value",
                   k.workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
                   k.expires_at, k.last_used_at, k.is_active,
                   k.created_at, k.updated_at,
                   k.revoked_at, k.revoked_reason
                FROM api_keys k
                WHERE k.key_hash = $1 AND k.is_active = true
               "#,
            self.key_hash
        )
        .fetch_optional(&mut *conn)
        .await?;

        if let Some(rec) = rec {
            let rate_limits = resolve_rate_limits_on_conn(
                &mut conn,
                rec.deployment_id,
                &rec.rate_limit_scheme_slug,
            )
            .await?;
            Ok(Some(ApiKey {
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
                rate_limits,
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
                    serde_json::from_value(rec.workspace_role_permissions.clone())
                        .unwrap_or_default()
                },
                expires_at: rec.expires_at,
                last_used_at: rec.last_used_at,
                is_active: rec.is_active.unwrap_or(true),
                created_at: rec.created_at.unwrap_or_else(chrono::Utc::now),
                updated_at: rec.updated_at.unwrap_or_else(chrono::Utc::now),
                revoked_at: rec.revoked_at,
                revoked_reason: rec.revoked_reason,
            }))
        } else {
            Ok(None)
        }
    }
}

pub struct GetApiKeyIdentifiersByHashQuery {
    pub key_hash: String,
}

impl GetApiKeyIdentifiersByHashQuery {
    pub fn new(key_hash: String) -> Self {
        Self { key_hash }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<ApiKeyWithIdentifers>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
            r#"SELECT k.id as id, k.app_slug as app_slug,
            k.permissions as "permissions: serde_json::Value",
            k.org_role_permissions as "org_role_permissions: serde_json::Value",
            k.workspace_role_permissions as "workspace_role_permissions: serde_json::Value",
            k.organization_id,
            k.workspace_id,
            k.organization_membership_id,
            k.workspace_membership_id,
            k.is_active as is_active,
            k.expires_at as expires_at
            FROM api_keys k
            WHERE k.key_hash = $1 AND k.is_active = true"#,
            self.key_hash
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(rec.map(|rec| ApiKeyWithIdentifers {
            id: rec.id,
            app_slug: rec.app_slug,
            permissions: serde_json::from_value(
                rec.permissions
                    .clone()
                    .unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default(),
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
            organization_id: rec.organization_id,
            workspace_id: rec.workspace_id,
            organization_membership_id: rec.organization_membership_id,
            workspace_membership_id: rec.workspace_membership_id,
            expires_at: rec.expires_at,
            is_active: rec.is_active.unwrap_or(true),
        }))
    }
}

pub struct SyncApiKeyRateLimitsForSchemeQuery {
    pub deployment_id: i64,
    pub scheme_slug: String,
    pub last_id: i64,
    pub batch_size: i64,
}

impl SyncApiKeyRateLimitsForSchemeQuery {
    pub fn new(deployment_id: i64, scheme_slug: String, last_id: i64, batch_size: i64) -> Self {
        Self {
            deployment_id,
            scheme_slug,
            last_id,
            batch_size,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let updated = sqlx::query!(
            r#"
            WITH target AS (
                SELECT id
                FROM api_keys
                WHERE deployment_id = $1
                  AND rate_limit_scheme_slug = $2
                  AND id > $3
                ORDER BY id
                LIMIT $4
            )
            UPDATE api_keys k
            SET updated_at = NOW()
            FROM target t
            WHERE k.id = t.id
            RETURNING k.id
            "#,
            self.deployment_id,
            self.scheme_slug,
            self.last_id,
            self.batch_size,
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(updated.into_iter().map(|r| r.id).collect())
    }
}

#[derive(Debug, Clone)]
pub struct OrganizationMembershipPermissions {
    pub organization_id: i64,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceMembershipPermissions {
    pub organization_id: i64,
    pub workspace_id: i64,
    pub permissions: Vec<String>,
}

pub struct GetOrganizationMembershipPermissionsQuery {
    pub membership_id: i64,
}

impl GetOrganizationMembershipPermissionsQuery {
    pub fn new(membership_id: i64) -> Self {
        Self { membership_id }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<OrganizationMembershipPermissions>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
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
            self.membership_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(rec.map(|r| OrganizationMembershipPermissions {
            organization_id: r.organization_id,
            permissions: serde_json::from_value(
                r.permissions.unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default(),
        }))
    }
}

pub struct GetWorkspaceMembershipPermissionsQuery {
    pub membership_id: i64,
}

impl GetWorkspaceMembershipPermissionsQuery {
    pub fn new(membership_id: i64) -> Self {
        Self { membership_id }
    }

    pub async fn execute_with_db<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<WorkspaceMembershipPermissions>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
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
            self.membership_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(rec.map(|r| WorkspaceMembershipPermissions {
            organization_id: r.organization_id,
            workspace_id: r.workspace_id,
            permissions: serde_json::from_value(
                r.permissions.unwrap_or_else(|| serde_json::json!([])),
            )
            .unwrap_or_default(),
        }))
    }
}

pub struct GetOrganizationMembershipIdByUserAndOrganizationQuery {
    pub user_id: i64,
    pub organization_id: i64,
}

impl GetOrganizationMembershipIdByUserAndOrganizationQuery {
    pub fn new(user_id: i64, organization_id: i64) -> Self {
        Self {
            user_id,
            organization_id,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
            r#"
            SELECT id
            FROM organization_memberships
            WHERE user_id = $1
              AND organization_id = $2
              AND deleted_at IS NULL
            LIMIT 1
            "#,
            self.user_id,
            self.organization_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(rec.map(|r| r.id))
    }
}

pub struct GetWorkspaceMembershipIdByUserAndWorkspaceQuery {
    pub user_id: i64,
    pub workspace_id: i64,
}

impl GetWorkspaceMembershipIdByUserAndWorkspaceQuery {
    pub fn new(user_id: i64, workspace_id: i64) -> Self {
        Self {
            user_id,
            workspace_id,
        }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Option<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
            r#"
            SELECT id
            FROM workspace_memberships
            WHERE user_id = $1
              AND workspace_id = $2
              AND deleted_at IS NULL
            LIMIT 1
            "#,
            self.user_id,
            self.workspace_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(rec.map(|r| r.id))
    }
}

pub struct GetOrganizationMembershipIdsByRoleQuery {
    pub role_id: i64,
}

impl GetOrganizationMembershipIdsByRoleQuery {
    pub fn new(role_id: i64) -> Self {
        Self { role_id }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let recs = sqlx::query!(
            r#"
            SELECT organization_membership_id as id
            FROM organization_membership_roles
            WHERE organization_role_id = $1
            "#,
            self.role_id
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(recs.into_iter().map(|r| r.id).collect())
    }
}

pub struct GetWorkspaceMembershipIdsByRoleQuery {
    pub role_id: i64,
}

impl GetWorkspaceMembershipIdsByRoleQuery {
    pub fn new(role_id: i64) -> Self {
        Self { role_id }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let recs = sqlx::query!(
            r#"
            SELECT workspace_membership_id as id
            FROM workspace_membership_roles
            WHERE workspace_role_id = $1
            "#,
            self.role_id
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(recs.into_iter().map(|r| r.id).collect())
    }
}

pub struct SyncApiKeyOrgRolePermissionsForMembershipsQuery {
    pub membership_ids: Vec<i64>,
}

impl SyncApiKeyOrgRolePermissionsForMembershipsQuery {
    pub fn new(membership_ids: Vec<i64>) -> Self {
        Self { membership_ids }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let updated = sqlx::query!(
            r#"
            WITH perms AS (
                SELECT
                    om.id as membership_id,
                    om.organization_id as organization_id,
                    COALESCE(
                        jsonb_agg(DISTINCT perm) FILTER (WHERE perm IS NOT NULL),
                        '[]'::jsonb
                    ) as permissions
                FROM organization_memberships om
                LEFT JOIN organization_membership_roles omr ON omr.organization_membership_id = om.id
                LEFT JOIN organization_roles orole ON omr.organization_role_id = orole.id
                LEFT JOIN LATERAL unnest(COALESCE(orole.permissions, ARRAY[]::text[])) perm ON true
                WHERE om.id = ANY($1)
                GROUP BY om.id, om.organization_id
            )
            UPDATE api_keys k
            SET org_role_permissions = perms.permissions,
                organization_id = perms.organization_id,
                updated_at = NOW()
            FROM perms
            WHERE k.organization_membership_id = perms.membership_id
            RETURNING k.id
            "#,
            &self.membership_ids
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(updated.into_iter().map(|r| r.id).collect())
    }
}

pub struct SyncApiKeyWorkspaceRolePermissionsForMembershipsQuery {
    pub membership_ids: Vec<i64>,
}

impl SyncApiKeyWorkspaceRolePermissionsForMembershipsQuery {
    pub fn new(membership_ids: Vec<i64>) -> Self {
        Self { membership_ids }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let updated = sqlx::query!(
            r#"
            WITH perms AS (
                SELECT
                    wm.id as membership_id,
                    wm.organization_id as organization_id,
                    wm.workspace_id as workspace_id,
                    COALESCE(
                        jsonb_agg(DISTINCT perm) FILTER (WHERE perm IS NOT NULL),
                        '[]'::jsonb
                    ) as permissions
                FROM workspace_memberships wm
                LEFT JOIN workspace_membership_roles wmr ON wmr.workspace_membership_id = wm.id
                LEFT JOIN workspace_roles wrole ON wmr.workspace_role_id = wrole.id
                LEFT JOIN LATERAL unnest(COALESCE(wrole.permissions, ARRAY[]::text[])) perm ON true
                WHERE wm.id = ANY($1)
                GROUP BY wm.id, wm.organization_id, wm.workspace_id
            )
            UPDATE api_keys k
            SET workspace_role_permissions = perms.permissions,
                organization_id = perms.organization_id,
                workspace_id = perms.workspace_id,
                updated_at = NOW()
            FROM perms
            WHERE k.workspace_membership_id = perms.membership_id
            RETURNING k.id
            "#,
            &self.membership_ids
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(updated.into_iter().map(|r| r.id).collect())
    }
}
