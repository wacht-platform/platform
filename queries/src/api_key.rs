use common::error::AppError;
use models::api_key::{ApiAuthApp, ApiKey, ApiKeyWithIdentifers};
use sqlx::Row;

fn map_api_auth_app(row: &sqlx::postgres::PgRow) -> ApiAuthApp {
    let permissions: Option<serde_json::Value> = row.get("permissions");
    let resources: Option<serde_json::Value> = row.get("resources");
    let rate_limits: serde_json::Value = row.get("rate_limits");

    ApiAuthApp {
        deployment_id: row.get("deployment_id"),
        user_id: row.get("user_id"),
        organization_id: row.get("organization_id"),
        workspace_id: row.get("workspace_id"),
        app_slug: row.get("app_slug"),
        name: row.get("name"),
        description: row.get("description"),
        is_active: row.get::<Option<bool>, _>("is_active").unwrap_or(true),
        key_prefix: row.get("key_prefix"),
        permissions: serde_json::from_value(permissions.unwrap_or_else(|| serde_json::json!([])))
            .unwrap_or_default(),
        resources: serde_json::from_value(resources.unwrap_or_else(|| serde_json::json!([])))
            .unwrap_or_default(),
        rate_limits: serde_json::from_value(rate_limits).unwrap_or_default(),
        rate_limit_scheme_slug: row.get("rate_limit_scheme_slug"),
        created_at: row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("created_at")
            .unwrap_or_else(chrono::Utc::now),
        updated_at: row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("updated_at")
            .unwrap_or_else(chrono::Utc::now),
        deleted_at: row.get("deleted_at"),
    }
}

fn map_api_key(row: &sqlx::postgres::PgRow) -> ApiKey {
    let permissions: Option<serde_json::Value> = row.get("permissions");
    let metadata: Option<serde_json::Value> = row.get("metadata");
    let org_role_permissions: Option<serde_json::Value> = row.get("org_role_permissions");
    let workspace_role_permissions: Option<serde_json::Value> = row.get("workspace_role_permissions");
    let rate_limits: serde_json::Value = row.get("rate_limits");

    ApiKey {
        id: row.get("id"),
        deployment_id: row.get("deployment_id"),
        app_slug: row.get("app_slug"),
        name: row.get("name"),
        key_prefix: row.get("key_prefix"),
        key_suffix: row.get("key_suffix"),
        key_hash: row.get("key_hash"),
        permissions: serde_json::from_value(permissions.unwrap_or_else(|| serde_json::json!([])))
            .unwrap_or_default(),
        metadata: metadata.unwrap_or_else(|| serde_json::json!({})),
        rate_limits: serde_json::from_value(rate_limits).unwrap_or_default(),
        rate_limit_scheme_slug: row.get("rate_limit_scheme_slug"),
        owner_user_id: row.get("owner_user_id"),
        organization_id: row.get("organization_id"),
        workspace_id: row.get("workspace_id"),
        organization_membership_id: row.get("organization_membership_id"),
        workspace_membership_id: row.get("workspace_membership_id"),
        org_role_permissions: serde_json::from_value(
            org_role_permissions.unwrap_or_else(|| serde_json::json!([])),
        )
        .unwrap_or_default(),
        workspace_role_permissions: serde_json::from_value(
            workspace_role_permissions.unwrap_or_else(|| serde_json::json!([])),
        )
        .unwrap_or_default(),
        expires_at: row.get("expires_at"),
        last_used_at: row.get("last_used_at"),
        is_active: row.get::<Option<bool>, _>("is_active").unwrap_or(true),
        created_at: row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("created_at")
            .unwrap_or_else(chrono::Utc::now),
        updated_at: row
            .get::<Option<chrono::DateTime<chrono::Utc>>, _>("updated_at")
            .unwrap_or_else(chrono::Utc::now),
        revoked_at: row.get("revoked_at"),
        revoked_reason: row.get("revoked_reason"),
    }
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ApiAuthApp>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query(
            r#"
            SELECT
                a.deployment_id, a.user_id, a.organization_id, a.workspace_id, a.app_slug,
                a.name, a.key_prefix, a.description, a.is_active, a.rate_limit_scheme_slug,
                a.permissions, a.resources, a.created_at, a.updated_at, a.deleted_at,
                COALESCE(rls.rules, '[]'::json) AS rate_limits
            FROM api_auth_apps a
            LEFT JOIN rate_limit_schemes rls
              ON rls.deployment_id = a.deployment_id
             AND rls.slug = a.rate_limit_scheme_slug
            WHERE a.deployment_id = $1
              AND a.deleted_at IS NULL
              AND ($2 OR a.is_active = true)
            ORDER BY a.created_at DESC
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.include_inactive)
        .fetch_all(executor)
        .await?;

        Ok(rows.iter().map(map_api_auth_app).collect())
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<ApiAuthApp>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT
                a.deployment_id, a.user_id, a.organization_id, a.workspace_id, a.app_slug,
                a.name, a.key_prefix, a.description, a.is_active, a.rate_limit_scheme_slug,
                a.permissions, a.resources, a.created_at, a.updated_at, a.deleted_at,
                COALESCE(rls.rules, '[]'::json) AS rate_limits
            FROM api_auth_apps a
            LEFT JOIN rate_limit_schemes rls
              ON rls.deployment_id = a.deployment_id
             AND rls.slug = a.rate_limit_scheme_slug
            WHERE a.deployment_id = $1 AND a.app_slug = $2 AND a.deleted_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.app_slug)
        .fetch_optional(executor)
        .await?;

        Ok(row.as_ref().map(map_api_auth_app))
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<ApiAuthApp>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT
                a.deployment_id, a.user_id, a.organization_id, a.workspace_id, a.app_slug,
                a.name, a.key_prefix, a.description, a.is_active, a.rate_limit_scheme_slug,
                a.permissions, a.resources, a.created_at, a.updated_at, a.deleted_at,
                COALESCE(rls.rules, '[]'::json) AS rate_limits
            FROM api_auth_apps a
            LEFT JOIN rate_limit_schemes rls
              ON rls.deployment_id = a.deployment_id
             AND rls.slug = a.rate_limit_scheme_slug
            WHERE a.deployment_id = $1 AND a.name = $2 AND a.deleted_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.name)
        .fetch_optional(executor)
        .await?;

        Ok(row.as_ref().map(map_api_auth_app))
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ApiKey>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query(
            r#"
            SELECT
                k.id, k.deployment_id, k.app_slug, k.name, k.key_prefix, k.key_suffix, k.key_hash,
                k.permissions, k.metadata, k.rate_limit_scheme_slug, k.owner_user_id,
                k.organization_id, k.workspace_id, k.organization_membership_id, k.workspace_membership_id,
                k.org_role_permissions, k.workspace_role_permissions,
                k.expires_at, k.last_used_at, k.is_active, k.created_at, k.updated_at,
                k.revoked_at, k.revoked_reason,
                COALESCE(rls.rules, '[]'::json) AS rate_limits
            FROM api_keys k
            LEFT JOIN rate_limit_schemes rls
              ON rls.deployment_id = k.deployment_id
             AND rls.slug = k.rate_limit_scheme_slug
            WHERE k.app_slug = $1
              AND k.deployment_id = $2
              AND ($3 OR k.is_active = true)
            ORDER BY k.created_at DESC
            "#,
        )
        .bind(&self.app_slug)
        .bind(self.deployment_id)
        .bind(self.include_inactive)
        .fetch_all(executor)
        .await?;

        Ok(rows.iter().map(map_api_key).collect())
    }
}

pub struct GetApiKeyByHashQuery {
    pub key_hash: String,
}

impl GetApiKeyByHashQuery {
    pub fn new(key_hash: String) -> Self {
        Self { key_hash }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<ApiKey>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT
                k.id, k.deployment_id, k.app_slug, k.name, k.key_prefix, k.key_suffix, k.key_hash,
                k.permissions, k.metadata, k.rate_limit_scheme_slug, k.owner_user_id,
                k.organization_id, k.workspace_id, k.organization_membership_id, k.workspace_membership_id,
                k.org_role_permissions, k.workspace_role_permissions,
                k.expires_at, k.last_used_at, k.is_active, k.created_at, k.updated_at,
                k.revoked_at, k.revoked_reason,
                COALESCE(rls.rules, '[]'::json) AS rate_limits
            FROM api_keys k
            LEFT JOIN rate_limit_schemes rls
              ON rls.deployment_id = k.deployment_id
             AND rls.slug = k.rate_limit_scheme_slug
            WHERE k.key_hash = $1 AND k.is_active = true
            "#,
        )
        .bind(&self.key_hash)
        .fetch_optional(executor)
        .await?;

        Ok(row.as_ref().map(map_api_key))
    }
}

pub struct GetApiKeyIdentifiersByHashQuery {
    pub key_hash: String,
}

impl GetApiKeyIdentifiersByHashQuery {
    pub fn new(key_hash: String) -> Self {
        Self { key_hash }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ApiKeyWithIdentifers>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_all(executor)
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<OrganizationMembershipPermissions>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
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

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<WorkspaceMembershipPermissions>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_optional(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let recs = sqlx::query!(
            r#"
            SELECT organization_membership_id as id
            FROM organization_membership_roles
            WHERE organization_role_id = $1
            "#,
            self.role_id
        )
        .fetch_all(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let recs = sqlx::query!(
            r#"
            SELECT workspace_membership_id as id
            FROM workspace_membership_roles
            WHERE workspace_role_id = $1
            "#,
            self.role_id
        )
        .fetch_all(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_all(executor)
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

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .fetch_all(executor)
        .await?;

        Ok(updated.into_iter().map(|r| r.id).collect())
    }
}
