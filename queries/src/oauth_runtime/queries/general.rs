use common::error::AppError;
use common::json_utils::{json_default, json_optional};
use sqlx::Row;

use crate::oauth_runtime::types::*;

fn normalize_permissions(permissions: &[String]) -> Vec<String> {
    permissions
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

fn parse_urn_resource_id(resource: &str, prefix: &str) -> Option<i64> {
    resource
        .strip_prefix(prefix)
        .and_then(|id| id.parse::<i64>().ok())
        .filter(|id| *id > 0)
}

pub struct ResolveOAuthAppByFqdnQuery {
    pub fqdn: String,
}

impl ResolveOAuthAppByFqdnQuery {
    pub fn new(fqdn: String) -> Self {
        Self { fqdn }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<RuntimeOAuthAppData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT id, deployment_id, slug, fqdn, supported_scopes, scope_definitions, allow_dynamic_client_registration
            FROM oauth_apps
            WHERE fqdn = $1
              AND is_active = TRUE
            "#,
        )
        .bind(&self.fqdn)
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| RuntimeOAuthAppData {
            id: r.get("id"),
            deployment_id: r.get("deployment_id"),
            slug: r.get("slug"),
            fqdn: r.get("fqdn"),
            supported_scopes: json_default(r.get("supported_scopes")),
            scope_definitions: json_default(r.get("scope_definitions")),
            allow_dynamic_client_registration: r.get("allow_dynamic_client_registration"),
        }))
    }
}

pub struct GetRuntimeDeploymentHostsByIdQuery {
    pub deployment_id: i64,
}

impl GetRuntimeDeploymentHostsByIdQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<RuntimeDeploymentHostsData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT backend_host, frontend_host
            FROM deployments
            WHERE id = $1
              AND deleted_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| RuntimeDeploymentHostsData {
            backend_host: r.get("backend_host"),
            frontend_host: r.get("frontend_host"),
        }))
    }
}

pub struct ResolveApiAuthAppSlugByApiKeyHashQuery {
    pub deployment_id: i64,
    pub key_hash: String,
}

impl ResolveApiAuthAppSlugByApiKeyHashQuery {
    pub fn new(deployment_id: i64, key_hash: String) -> Self {
        Self {
            deployment_id,
            key_hash,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT app_slug
            FROM api_keys
            WHERE deployment_id = $1
              AND key_hash = $2
              AND is_active = TRUE
              AND revoked_at IS NULL
              AND (expires_at IS NULL OR expires_at > NOW())
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.key_hash)
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| r.get("app_slug")))
    }
}

pub struct GetRuntimeApiAuthAppSlugByUserIdQuery {
    pub deployment_id: i64,
    pub user_id: i64,
}

impl GetRuntimeApiAuthAppSlugByUserIdQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT app_slug
            FROM api_auth_apps
            WHERE deployment_id = $1
              AND user_id = $2
              AND deleted_at IS NULL
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.user_id)
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| r.get("app_slug")))
    }
}

pub struct GetRuntimeOAuthClientByClientIdQuery {
    pub oauth_app_id: i64,
    pub client_id: String,
}

impl GetRuntimeOAuthClientByClientIdQuery {
    pub fn new(oauth_app_id: i64, client_id: String) -> Self {
        Self {
            oauth_app_id,
            client_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<RuntimeOAuthClientData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                oauth_app_id,
                client_id,
                client_secret_hash,
                client_secret_encrypted,
                registration_access_token_hash,
                client_auth_method,
                grant_types,
                redirect_uris,
                token_endpoint_auth_signing_alg,
                jwks,
                client_name,
                client_uri,
                logo_uri,
                tos_uri,
                policy_uri,
                contacts,
                software_id,
                software_version,
                is_active,
                created_at,
                post_logout_redirect_uris,
                id_token_signing_alg,
                access_token_format,
                access_token_ttl_seconds,
                skip_consent
            FROM oauth_clients
            WHERE oauth_app_id = $1
              AND client_id = $2
            "#,
        )
        .bind(self.oauth_app_id)
        .bind(&self.client_id)
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| RuntimeOAuthClientData {
            id: r.get("id"),
            oauth_app_id: r.get("oauth_app_id"),
            client_id: r.get("client_id"),
            client_secret_hash: r.get("client_secret_hash"),
            client_secret_encrypted: r.get("client_secret_encrypted"),
            registration_access_token_hash: r.get("registration_access_token_hash"),
            client_auth_method: r.get("client_auth_method"),
            grant_types: json_default(r.get("grant_types")),
            redirect_uris: json_default(r.get("redirect_uris")),
            token_endpoint_auth_signing_alg: r.get("token_endpoint_auth_signing_alg"),
            jwks: json_optional(r.get("jwks")),
            client_name: r.get("client_name"),
            client_uri: r.get("client_uri"),
            logo_uri: r.get("logo_uri"),
            tos_uri: r.get("tos_uri"),
            policy_uri: r.get("policy_uri"),
            contacts: json_optional(r.get("contacts")).unwrap_or_default(),
            software_id: r.get("software_id"),
            software_version: r.get("software_version"),
            is_active: r.get("is_active"),
            created_at: r.get("created_at"),
            post_logout_redirect_uris: json_default(r.get("post_logout_redirect_uris")),
            id_token_signing_alg: r.get("id_token_signing_alg"),
            access_token_format: r.get("access_token_format"),
            access_token_ttl_seconds: r.get("access_token_ttl_seconds"),
            skip_consent: r.get("skip_consent"),
        }))
    }
}

pub struct ListActiveRuntimeOAuthGrantsQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub app_slug: String,
}

impl ListActiveRuntimeOAuthGrantsQuery {
    pub fn new(deployment_id: i64, oauth_client_id: i64, app_slug: String) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
            app_slug,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<RuntimeOAuthGrantData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query(
            r#"
            SELECT scopes, resource, granted_resource
            FROM oauth_client_grants
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND app_slug = $3
              AND status = 'active'
              AND (expires_at IS NULL OR expires_at > NOW())
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(&self.app_slug)
        .fetch_all(executor)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| RuntimeOAuthGrantData {
                scopes: json_default(r.get("scopes")),
                resource: r.get("resource"),
                granted_resource: r.get("granted_resource"),
            })
            .collect())
    }
}

pub struct GetRuntimeAuthorizationCodeForExchangeQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub code_hash: String,
}

impl GetRuntimeAuthorizationCodeForExchangeQuery {
    pub fn new(deployment_id: i64, oauth_client_id: i64, code_hash: String) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
            code_hash,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<RuntimeAuthorizationCodeData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT
                c.id,
                c.oauth_grant_id,
                c.app_slug,
                c.redirect_uri,
                c.pkce_code_challenge,
                c.pkce_code_challenge_method,
                c.scopes,
                c.resource,
                c.granted_resource,
                c.nonce,
                c.auth_time,
                c.session_id,
                g.granted_by_user_id AS "user_id?: i64"
            FROM oauth_authorization_codes c
            LEFT JOIN oauth_client_grants g ON g.id = c.oauth_grant_id
            WHERE c.deployment_id = $1
              AND c.oauth_client_id = $2
              AND c.code_hash = $3
              AND c.consumed_at IS NULL
              AND c.expires_at > NOW()
            "#,
            self.deployment_id,
            self.oauth_client_id,
            self.code_hash,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| RuntimeAuthorizationCodeData {
            id: r.id,
            oauth_grant_id: r.oauth_grant_id,
            app_slug: r.app_slug,
            redirect_uri: r.redirect_uri,
            pkce_code_challenge: r.pkce_code_challenge,
            pkce_code_challenge_method: r.pkce_code_challenge_method,
            scopes: json_default(r.scopes),
            resource: r.resource,
            granted_resource: r.granted_resource,
            nonce: r.nonce,
            auth_time: r.auth_time,
            session_id: r.session_id,
            user_id: r.user_id,
        }))
    }
}

/// Returns `signins.created_at` for the (session, user) pair — the value the
/// OIDC `auth_time` claim is supposed to reflect.
pub struct GetSigninAuthTimeQuery {
    pub session_id: i64,
    pub user_id: i64,
}

impl GetSigninAuthTimeQuery {
    pub fn new(session_id: i64, user_id: i64) -> Self {
        Self {
            session_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT created_at
              FROM signins
             WHERE session_id = $1
               AND user_id = $2
               AND deleted_at IS NULL
             ORDER BY created_at DESC
             LIMIT 1
            "#,
            self.session_id,
            self.user_id,
        )
        .fetch_optional(executor)
        .await?;
        Ok(row.map(|r| r.created_at))
    }
}

/// onto the auth code we're about to issue. Returns `None` if the user has
/// no active session — typical for service flows or expired sessions.
pub struct GetActiveSessionForUserQuery {
    pub deployment_id: i64,
    pub user_id: i64,
}

impl GetActiveSessionForUserQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT s.id AS "id!"
              FROM sessions s
              JOIN signins si ON si.session_id = s.id
             WHERE si.user_id = $1
               AND s.deleted_at IS NULL
               AND si.deleted_at IS NULL
               AND (s.deployment_id IS NULL OR s.deployment_id = $2)
             ORDER BY s.updated_at DESC
             LIMIT 1
            "#,
            self.user_id,
            self.deployment_id,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| r.id))
    }
}

pub struct GetRuntimeRefreshTokenForExchangeQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub token_hash: String,
}

impl GetRuntimeRefreshTokenForExchangeQuery {
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
    ) -> Result<Option<RuntimeRefreshTokenData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        // Intentionally surface revoked / expired / dead-session rows so the
        // application layer can detect refresh-token replay (RFC 6749 §10.4 /
        // OAuth 2.1 §6.1). Filtering revoked rows here would hide the very
        // case where replay matters: a stolen refresh token that's already
        // been rotated. Application layer checks status fields below.
        let row = sqlx::query!(
            r#"
            SELECT
                rt.id,
                rt.oauth_grant_id,
                rt.app_slug,
                rt.replaced_by_token_id,
                rt.revoked_at,
                rt.expires_at,
                rt.scopes,
                rt.resource,
                rt.granted_resource,
                rt.session_id,
                s.deleted_at AS "session_deleted_at: chrono::DateTime<chrono::Utc>",
                g.granted_by_user_id AS "user_id?: i64"
            FROM oauth_refresh_tokens rt
            LEFT JOIN sessions s ON s.id = rt.session_id
            LEFT JOIN oauth_client_grants g ON g.id = rt.oauth_grant_id
            WHERE rt.deployment_id = $1
              AND rt.oauth_client_id = $2
              AND rt.token_hash = $3
            "#,
            self.deployment_id,
            self.oauth_client_id,
            self.token_hash,
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| RuntimeRefreshTokenData {
            id: r.id,
            oauth_grant_id: r.oauth_grant_id,
            app_slug: r.app_slug,
            replaced_by_token_id: r.replaced_by_token_id,
            revoked_at: r.revoked_at,
            expires_at: r.expires_at,
            scopes: json_default(r.scopes),
            resource: r.resource,
            granted_resource: r.granted_resource,
            session_id: r.session_id,
            session_deleted_at: r.session_deleted_at,
            user_id: r.user_id,
        }))
    }
}

pub struct ResolveRuntimeOAuthGrantQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub grant_id: Option<i64>,
    pub app_slug: Option<String>,
    pub scopes: Vec<String>,
    pub granted_resource: Option<String>,
}

impl ResolveRuntimeOAuthGrantQuery {
    pub fn by_grant_id(deployment_id: i64, oauth_client_id: i64, grant_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
            grant_id: Some(grant_id),
            app_slug: None,
            scopes: Vec::new(),
            granted_resource: None,
        }
    }

    pub fn by_scope_match(
        deployment_id: i64,
        oauth_client_id: i64,
        app_slug: String,
        scopes: Vec<String>,
        granted_resource: Option<String>,
    ) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
            grant_id: None,
            app_slug: Some(app_slug),
            scopes,
            granted_resource,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<RuntimeOAuthGrantResolution, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let scopes_json = serde_json::to_value(&self.scopes)?;
        let row = sqlx::query!(
            r#"
            WITH matched AS (
              SELECT id, status, expires_at, updated_at
              FROM oauth_client_grants
              WHERE deployment_id = $1
                AND oauth_client_id = $2
                AND (
                  ($3::bigint IS NOT NULL AND id = $3)
                  OR (
                    $3::bigint IS NULL
                    AND app_slug = $4
                    AND ($5::text IS NULL OR granted_resource = $5)
                    AND scopes @> $6::jsonb
                  )
                )
            )
            SELECT
              (
                SELECT id
                FROM matched
                WHERE status = 'active'
                  AND (expires_at IS NULL OR expires_at > NOW())
                ORDER BY updated_at DESC
                LIMIT 1
              ) AS active_grant_id,
              EXISTS (
                SELECT 1
                FROM matched
                WHERE status = 'active'
                  AND (expires_at IS NULL OR expires_at > NOW())
              ) AS active,
              EXISTS (
                SELECT 1
                FROM matched
                WHERE status = 'revoked'
              ) AS revoked
            "#,
            self.deployment_id,
            self.oauth_client_id,
            self.grant_id,
            self.app_slug,
            self.granted_resource,
            scopes_json
        )
        .fetch_one(executor)
        .await?;

        Ok(RuntimeOAuthGrantResolution {
            active_grant_id: row.active_grant_id,
            active: row.active.unwrap_or(false),
            revoked: row.revoked.unwrap_or(false),
        })
    }
}

pub struct GetRuntimeApiAuthUserIdByAppSlugQuery {
    pub deployment_id: i64,
    pub app_slug: String,
}

impl GetRuntimeApiAuthUserIdByAppSlugQuery {
    pub fn new(deployment_id: i64, app_slug: String) -> Self {
        Self {
            deployment_id,
            app_slug,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT user_id
            FROM api_auth_apps
            WHERE deployment_id = $1
              AND app_slug = $2
              AND deleted_at IS NULL
            LIMIT 1
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.app_slug)
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| r.get("user_id")))
    }
}

pub struct ValidateRuntimeResourceEntitlementQuery {
    pub deployment_id: i64,
    pub user_id: i64,
    pub resource: String,
    pub required_permissions: Vec<String>,
}

impl ValidateRuntimeResourceEntitlementQuery {
    pub fn new(
        deployment_id: i64,
        user_id: i64,
        resource: String,
        required_permissions: Vec<String>,
    ) -> Self {
        Self {
            deployment_id,
            user_id,
            resource,
            required_permissions,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<bool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let required_permissions = normalize_permissions(&self.required_permissions);

        if let Some(resource_user_id) = parse_urn_resource_id(&self.resource, "urn:wacht:user:") {
            return Ok(resource_user_id == self.user_id && required_permissions.is_empty());
        }

        if let Some(org_id) = parse_urn_resource_id(&self.resource, "urn:wacht:organization:") {
            let row = sqlx::query(
                r#"
                SELECT
                  (
                    COUNT(DISTINCT om.id) > 0
                    AND COALESCE(
                      array_agg(DISTINCT perm.permission) FILTER (WHERE perm.permission IS NOT NULL),
                      ARRAY[]::text[]
                    ) @> $4::text[]
                  ) AS entitled
                FROM organization_memberships om
                INNER JOIN organizations o
                  ON o.id = om.organization_id
                 AND o.deployment_id = $1
                 AND o.deleted_at IS NULL
                LEFT JOIN organization_membership_roles omr
                  ON omr.organization_membership_id = om.id
                LEFT JOIN organization_roles r
                  ON r.id = omr.organization_role_id
                LEFT JOIN LATERAL unnest(COALESCE(r.permissions, ARRAY[]::text[])) AS perm(permission)
                  ON TRUE
                WHERE om.organization_id = $2
                  AND om.user_id = $3
                  AND om.deleted_at IS NULL
                "#,
            )
            .bind(self.deployment_id)
            .bind(org_id)
            .bind(self.user_id)
            .bind(required_permissions)
            .fetch_one(executor)
            .await?;
            return Ok(row.get("entitled"));
        }

        if let Some(workspace_id) = parse_urn_resource_id(&self.resource, "urn:wacht:workspace:") {
            let row = sqlx::query(
                r#"
                SELECT
                  (
                    COUNT(DISTINCT wm.id) > 0
                    AND COALESCE(
                      array_agg(DISTINCT perm.permission) FILTER (WHERE perm.permission IS NOT NULL),
                      ARRAY[]::text[]
                    ) @> $4::text[]
                  ) AS entitled
                FROM workspace_memberships wm
                INNER JOIN workspaces w
                  ON w.id = wm.workspace_id
                 AND w.deployment_id = $1
                 AND w.deleted_at IS NULL
                LEFT JOIN workspace_membership_roles wmr
                  ON wmr.workspace_membership_id = wm.id
                LEFT JOIN workspace_roles r
                  ON r.id = wmr.workspace_role_id
                LEFT JOIN LATERAL unnest(COALESCE(r.permissions, ARRAY[]::text[])) AS perm(permission)
                  ON TRUE
                WHERE wm.workspace_id = $2
                  AND wm.user_id = $3
                  AND wm.deleted_at IS NULL
                "#,
            )
            .bind(self.deployment_id)
            .bind(workspace_id)
            .bind(self.user_id)
            .bind(required_permissions)
            .fetch_one(executor)
            .await?;
            return Ok(row.get("entitled"));
        }

        Ok(false)
    }
}
