use common::error::AppError;
use common::json_utils::json_default;
use sqlx::Row;

use crate::oauth_runtime::types::*;

pub struct GetRuntimeAccessTokenByHashQuery {
    pub deployment_id: i64,
    pub token_hash: String,
}

impl GetRuntimeAccessTokenByHashQuery {
    pub fn new(deployment_id: i64, token_hash: String) -> Self {
        Self {
            deployment_id,
            token_hash,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<RuntimeAccessTokenData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT
                t.oauth_grant_id,
                t.app_slug,
                t.oauth_client_id,
                c.client_id,
                t.scopes,
                t.resource,
                t.granted_resource,
                t.expires_at,
                t.revoked_at
            FROM oauth_access_tokens t
            INNER JOIN oauth_clients c
              ON c.id = t.oauth_client_id
            WHERE t.deployment_id = $1
              AND t.token_hash = $2
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.token_hash)
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| RuntimeAccessTokenData {
            oauth_grant_id: r.get("oauth_grant_id"),
            app_slug: r.get("app_slug"),
            oauth_client_id: r.get("oauth_client_id"),
            client_id: r.get("client_id"),
            scopes: json_default(r.get("scopes")),
            resource: r.get("resource"),
            granted_resource: r.get("granted_resource"),
            expires_at: r.get("expires_at"),
            revoked_at: r.get("revoked_at"),
        }))
    }
}

pub struct GetRuntimeIntrospectionDataQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl GetRuntimeIntrospectionDataQuery {
    pub fn new(deployment_id: i64, oauth_client_id: i64, token_hash: String) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
            token_hash,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<RuntimeIntrospectionData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            WITH token_row AS (
                SELECT
                    t.app_slug,
                    t.oauth_grant_id,
                    t.oauth_client_id,
                    c.oauth_app_id,
                    c.client_id,
                    t.scopes,
                    t.resource,
                    t.granted_resource,
                    t.created_at,
                    t.expires_at,
                    t.revoked_at
                FROM oauth_access_tokens t
                INNER JOIN oauth_clients c
                    ON c.id = t.oauth_client_id
                WHERE t.deployment_id = $1
                  AND t.oauth_client_id = $2
                  AND t.token_hash = $3
                LIMIT 1
            ),
            grant_status AS (
                SELECT
                    EXISTS (
                        SELECT 1
                        FROM oauth_client_grants g
                        INNER JOIN token_row tr ON TRUE
                        WHERE g.deployment_id = $1
                          AND g.oauth_client_id = tr.oauth_client_id
                          AND tr.oauth_grant_id IS NOT NULL
                          AND g.id = tr.oauth_grant_id
                          AND g.status = 'active'
                          AND (g.expires_at IS NULL OR g.expires_at > NOW())
                    ) AS valid
            ),
            api_auth_user AS (
                SELECT aa.user_id
                FROM api_auth_apps aa
                INNER JOIN token_row tr ON TRUE
                WHERE aa.deployment_id = $1
                  AND aa.app_slug = tr.app_slug
                  AND aa.deleted_at IS NULL
                LIMIT 1
            ),
            required_permissions AS (
                SELECT DISTINCT perm.permission
                FROM token_row tr
                INNER JOIN oauth_apps oa
                    ON oa.deployment_id = $1
                   AND oa.id = tr.oauth_app_id
                INNER JOIN LATERAL jsonb_array_elements(COALESCE(oa.scope_definitions, '[]'::jsonb)) AS def(scope_def) ON TRUE
                INNER JOIN LATERAL (
                    VALUES (
                        LOWER(TRIM(COALESCE(def.scope_def->>'category', ''))),
                        CASE
                            WHEN tr.granted_resource LIKE 'urn:wacht:organization:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'organization_permission', '')), '')
                            WHEN tr.granted_resource LIKE 'urn:wacht:workspace:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'workspace_permission', '')), '')
                            ELSE NULL
                        END
                    )
                ) AS perm(category, permission) ON TRUE
                WHERE (def.scope_def->>'scope') IN (SELECT jsonb_array_elements_text(tr.scopes))
                  AND (
                      (tr.granted_resource LIKE 'urn:wacht:organization:%' AND perm.category = 'organization') OR
                      (tr.granted_resource LIKE 'urn:wacht:workspace:%' AND perm.category = 'workspace')
                  )
                  AND perm.permission IS NOT NULL
            ),
            required_permission_array AS (
                SELECT COALESCE(array_agg(permission), ARRAY[]::text[]) AS permissions
                FROM required_permissions
            )
            SELECT
                tr.oauth_grant_id,
                tr.client_id,
                tr.app_slug,
                tr.scopes as "scopes: serde_json::Value",
                tr.resource,
                tr.granted_resource,
                tr.created_at,
                tr.expires_at,
                (
                    tr.revoked_at IS NULL
                    AND tr.expires_at > NOW()
                    AND COALESCE(gs.valid, FALSE)
                    AND CASE
                        WHEN tr.granted_resource IS NULL THEN TRUE
                        WHEN split_part(tr.granted_resource, ':', 1) = 'urn'
                          AND split_part(tr.granted_resource, ':', 2) = 'wacht'
                          AND split_part(tr.granted_resource, ':', 3) = 'user'
                          AND split_part(tr.granted_resource, ':', 4) <> ''
                          AND split_part(tr.granted_resource, ':', 4) !~ '[^0-9]'
                        THEN
                            EXISTS (
                                SELECT 1
                                FROM api_auth_user au
                                WHERE au.user_id = split_part(tr.granted_resource, ':', 4)::bigint
                                  AND cardinality((SELECT permissions FROM required_permission_array)) = 0
                            )
                        WHEN split_part(tr.granted_resource, ':', 1) = 'urn'
                          AND split_part(tr.granted_resource, ':', 2) = 'wacht'
                          AND split_part(tr.granted_resource, ':', 3) = 'organization'
                          AND split_part(tr.granted_resource, ':', 4) <> ''
                          AND split_part(tr.granted_resource, ':', 4) !~ '[^0-9]'
                        THEN
                            EXISTS (
                                SELECT 1
                                FROM (
                                    SELECT
                                        COUNT(DISTINCT om.id) AS membership_count,
                                        COALESCE(
                                            array_agg(DISTINCT p.permission) FILTER (WHERE p.permission IS NOT NULL),
                                            ARRAY[]::text[]
                                        ) AS permissions
                                    FROM api_auth_user au
                                    INNER JOIN organization_memberships om
                                        ON om.user_id = au.user_id
                                       AND om.organization_id = split_part(tr.granted_resource, ':', 4)::bigint
                                       AND om.deleted_at IS NULL
                                    INNER JOIN organizations o
                                        ON o.id = om.organization_id
                                       AND o.deployment_id = $1
                                       AND o.deleted_at IS NULL
                                    LEFT JOIN organization_membership_roles omr
                                        ON omr.organization_membership_id = om.id
                                    LEFT JOIN organization_roles r
                                        ON r.id = omr.organization_role_id
                                    LEFT JOIN LATERAL unnest(COALESCE(r.permissions, ARRAY[]::text[])) AS p(permission)
                                        ON TRUE
                                ) perms
                                WHERE perms.membership_count > 0
                                  AND perms.permissions @> (SELECT permissions FROM required_permission_array)
                            )
                        WHEN split_part(tr.granted_resource, ':', 1) = 'urn'
                          AND split_part(tr.granted_resource, ':', 2) = 'wacht'
                          AND split_part(tr.granted_resource, ':', 3) = 'workspace'
                          AND split_part(tr.granted_resource, ':', 4) <> ''
                          AND split_part(tr.granted_resource, ':', 4) !~ '[^0-9]'
                        THEN
                            EXISTS (
                                SELECT 1
                                FROM (
                                    SELECT
                                        COUNT(DISTINCT wm.id) AS membership_count,
                                        COALESCE(
                                            array_agg(DISTINCT p.permission) FILTER (WHERE p.permission IS NOT NULL),
                                            ARRAY[]::text[]
                                        ) AS permissions
                                    FROM api_auth_user au
                                    INNER JOIN workspace_memberships wm
                                        ON wm.user_id = au.user_id
                                       AND wm.workspace_id = split_part(tr.granted_resource, ':', 4)::bigint
                                       AND wm.deleted_at IS NULL
                                    INNER JOIN workspaces w
                                        ON w.id = wm.workspace_id
                                       AND w.deployment_id = $1
                                       AND w.deleted_at IS NULL
                                    LEFT JOIN workspace_membership_roles wmr
                                        ON wmr.workspace_membership_id = wm.id
                                    LEFT JOIN workspace_roles r
                                        ON r.id = wmr.workspace_role_id
                                    LEFT JOIN LATERAL unnest(COALESCE(r.permissions, ARRAY[]::text[])) AS p(permission)
                                        ON TRUE
                                ) perms
                                WHERE perms.membership_count > 0
                                  AND perms.permissions @> (SELECT permissions FROM required_permission_array)
                            )
                        ELSE FALSE
                    END
                ) AS active
            FROM token_row tr
            LEFT JOIN grant_status gs ON TRUE
            "#,
            self.deployment_id,
            self.oauth_client_id,
            self.token_hash
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| RuntimeIntrospectionData {
            active: r.active.unwrap_or(false),
            oauth_grant_id: r.oauth_grant_id,
            client_id: r.client_id,
            app_slug: r.app_slug,
            scopes: json_default(r.scopes),
            resource: r.resource,
            granted_resource: r.granted_resource,
            issued_at: r.created_at,
            expires_at: r.expires_at,
        }))
    }
}

pub struct GetGatewayOAuthAccessTokenByHashQuery {
    pub token_hash: String,
}

impl GetGatewayOAuthAccessTokenByHashQuery {
    pub fn new(token_hash: String) -> Self {
        Self { token_hash }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<GatewayOAuthAccessTokenData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            WITH token_row AS (
                SELECT
                    t.deployment_id,
                    t.oauth_grant_id,
                    t.oauth_client_id,
                    c.client_id,
                    t.app_slug,
                    oa.fqdn AS oauth_issuer,
                    t.scopes,
                    t.resource,
                    t.granted_resource,
                    t.expires_at,
                    t.revoked_at,
                    aa.user_id,
                    aa.is_active AS api_auth_app_is_active,
                    aa.rate_limit_scheme_slug,
                    oa.scope_definitions
                FROM oauth_access_tokens t
                INNER JOIN oauth_clients c
                    ON c.id = t.oauth_client_id
                INNER JOIN oauth_apps oa
                    ON oa.id = c.oauth_app_id
                   AND oa.deployment_id = t.deployment_id
                INNER JOIN api_auth_apps aa
                    ON aa.deployment_id = t.deployment_id
                   AND aa.app_slug = t.app_slug
                   AND aa.deleted_at IS NULL
                WHERE t.token_hash = $1
                LIMIT 1
            ),
            grant_status AS (
                SELECT
                    EXISTS (
                        SELECT 1
                        FROM oauth_client_grants g
                        INNER JOIN token_row tr ON TRUE
                        WHERE g.deployment_id = tr.deployment_id
                          AND g.oauth_client_id = tr.oauth_client_id
                          AND tr.oauth_grant_id IS NOT NULL
                          AND g.id = tr.oauth_grant_id
                          AND g.status = 'active'
                          AND (g.expires_at IS NULL OR g.expires_at > NOW())
                    ) AS valid
            ),
            required_permissions AS (
                SELECT DISTINCT perm.permission
                FROM token_row tr
                INNER JOIN LATERAL jsonb_array_elements(COALESCE(tr.scope_definitions, '[]'::jsonb))
                    AS def(scope_def) ON TRUE
                INNER JOIN LATERAL (
                    VALUES (
                        LOWER(TRIM(COALESCE(def.scope_def->>'category', ''))),
                        CASE
                            WHEN tr.granted_resource LIKE 'urn:wacht:organization:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'organization_permission', '')), '')
                            WHEN tr.granted_resource LIKE 'urn:wacht:workspace:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'workspace_permission', '')), '')
                            ELSE NULL
                        END
                    )
                ) AS perm(category, permission) ON TRUE
                WHERE (def.scope_def->>'scope') IN (SELECT jsonb_array_elements_text(tr.scopes))
                  AND (
                      (tr.granted_resource LIKE 'urn:wacht:organization:%' AND perm.category = 'organization') OR
                      (tr.granted_resource LIKE 'urn:wacht:workspace:%' AND perm.category = 'workspace')
                  )
                  AND perm.permission IS NOT NULL
            ),
            required_permission_array AS (
                SELECT COALESCE(array_agg(permission), ARRAY[]::text[]) AS permissions
                FROM required_permissions
            )
            SELECT
                tr.deployment_id,
                tr.oauth_grant_id,
                tr.oauth_client_id,
                tr.client_id,
                tr.app_slug,
                tr.oauth_issuer,
                tr.user_id as "owner_user_id?",
                tr.scopes as "scopes!: serde_json::Value",
                tr.resource,
                tr.granted_resource,
                tr.expires_at,
                tr.rate_limit_scheme_slug,
                tr.scope_definitions as "scope_definitions!: serde_json::Value",
                (
                    tr.api_auth_app_is_active IS TRUE
                    AND tr.revoked_at IS NULL
                    AND tr.expires_at > NOW()
                    AND COALESCE(gs.valid, FALSE)
                    AND CASE
                        WHEN tr.granted_resource IS NULL THEN TRUE
                        WHEN split_part(tr.granted_resource, ':', 1) = 'urn'
                          AND split_part(tr.granted_resource, ':', 2) = 'wacht'
                          AND split_part(tr.granted_resource, ':', 3) = 'user'
                          AND split_part(tr.granted_resource, ':', 4) <> ''
                          AND split_part(tr.granted_resource, ':', 4) !~ '[^0-9]'
                        THEN
                            tr.user_id = split_part(tr.granted_resource, ':', 4)::bigint
                            AND cardinality((SELECT permissions FROM required_permission_array)) = 0
                        WHEN split_part(tr.granted_resource, ':', 1) = 'urn'
                          AND split_part(tr.granted_resource, ':', 2) = 'wacht'
                          AND split_part(tr.granted_resource, ':', 3) = 'organization'
                          AND split_part(tr.granted_resource, ':', 4) <> ''
                          AND split_part(tr.granted_resource, ':', 4) !~ '[^0-9]'
                        THEN
                            EXISTS (
                                SELECT 1
                                FROM (
                                    SELECT
                                        COUNT(DISTINCT om.id) AS membership_count,
                                        COALESCE(
                                            array_agg(DISTINCT p.permission) FILTER (WHERE p.permission IS NOT NULL),
                                            ARRAY[]::text[]
                                        ) AS permissions
                                    FROM organization_memberships om
                                    INNER JOIN organizations o
                                        ON o.id = om.organization_id
                                       AND o.deployment_id = tr.deployment_id
                                       AND o.deleted_at IS NULL
                                    LEFT JOIN organization_membership_roles omr
                                        ON omr.organization_membership_id = om.id
                                    LEFT JOIN organization_roles r
                                        ON r.id = omr.organization_role_id
                                    LEFT JOIN LATERAL unnest(COALESCE(r.permissions, ARRAY[]::text[])) AS p(permission)
                                        ON TRUE
                                    WHERE om.user_id = tr.user_id
                                      AND om.organization_id = split_part(tr.granted_resource, ':', 4)::bigint
                                      AND om.deleted_at IS NULL
                                ) perms
                                WHERE perms.membership_count > 0
                                  AND perms.permissions @> (SELECT permissions FROM required_permission_array)
                            )
                        WHEN split_part(tr.granted_resource, ':', 1) = 'urn'
                          AND split_part(tr.granted_resource, ':', 2) = 'wacht'
                          AND split_part(tr.granted_resource, ':', 3) = 'workspace'
                          AND split_part(tr.granted_resource, ':', 4) <> ''
                          AND split_part(tr.granted_resource, ':', 4) !~ '[^0-9]'
                        THEN
                            EXISTS (
                                SELECT 1
                                FROM (
                                    SELECT
                                        COUNT(DISTINCT wm.id) AS membership_count,
                                        COALESCE(
                                            array_agg(DISTINCT p.permission) FILTER (WHERE p.permission IS NOT NULL),
                                            ARRAY[]::text[]
                                        ) AS permissions
                                    FROM workspace_memberships wm
                                    INNER JOIN workspaces w
                                        ON w.id = wm.workspace_id
                                       AND w.deployment_id = tr.deployment_id
                                       AND w.deleted_at IS NULL
                                    LEFT JOIN workspace_membership_roles wmr
                                        ON wmr.workspace_membership_id = wm.id
                                    LEFT JOIN workspace_roles r
                                        ON r.id = wmr.workspace_role_id
                                    LEFT JOIN LATERAL unnest(COALESCE(r.permissions, ARRAY[]::text[])) AS p(permission)
                                        ON TRUE
                                    WHERE wm.user_id = tr.user_id
                                      AND wm.workspace_id = split_part(tr.granted_resource, ':', 4)::bigint
                                      AND wm.deleted_at IS NULL
                                ) perms
                                WHERE perms.membership_count > 0
                                  AND perms.permissions @> (SELECT permissions FROM required_permission_array)
                            )
                        ELSE FALSE
                    END
                ) AS "active!"
            FROM token_row tr
            LEFT JOIN grant_status gs ON TRUE
            "#,
            self.token_hash
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| GatewayOAuthAccessTokenData {
            deployment_id: r.deployment_id,
            oauth_grant_id: r.oauth_grant_id,
            oauth_client_id: r.oauth_client_id,
            client_id: r.client_id,
            app_slug: r.app_slug,
            oauth_issuer: format!("https://{}", r.oauth_issuer),
            owner_user_id: r.owner_user_id,
            scopes: json_default(r.scopes),
            resource: r.resource,
            granted_resource: r.granted_resource,
            expires_at: r.expires_at,
            rate_limits: vec![],
            rate_limit_scheme_slug: r.rate_limit_scheme_slug,
            scope_definitions: json_default(r.scope_definitions),
            active: r.active,
        }))
    }
}
