use crate::Query;
use common::error::AppError;
use common::state::AppState;
use models::api_key::{JwksDocument, OAuthScopeDefinition, RateLimit};
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOAuthAppData {
    pub id: i64,
    pub deployment_id: i64,
    pub slug: String,
    pub fqdn: String,
    pub supported_scopes: Vec<String>,
    pub scope_definitions: Vec<OAuthScopeDefinition>,
    pub allow_dynamic_client_registration: bool,
}

impl RuntimeOAuthAppData {
    pub fn active_scopes(&self) -> Vec<String> {
        let mut archived = std::collections::HashSet::<String>::new();
        for definition in &self.scope_definitions {
            if definition.archived {
                archived.insert(definition.scope.clone());
            }
        }

        self.supported_scopes
            .iter()
            .filter(|scope| !archived.contains((*scope).as_str()))
            .cloned()
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDeploymentHostsData {
    pub backend_host: String,
    pub frontend_host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOAuthClientData {
    pub id: i64,
    pub oauth_app_id: i64,
    pub client_id: String,
    pub client_secret_hash: Option<String>,
    pub client_secret_encrypted: Option<String>,
    pub registration_access_token_hash: Option<String>,
    pub client_auth_method: String,
    pub grant_types: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Vec<String>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOAuthGrantData {
    pub scopes: Vec<String>,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeAuthorizationCodeData {
    pub id: i64,
    pub oauth_grant_id: Option<i64>,
    pub app_slug: String,
    pub redirect_uri: String,
    pub pkce_code_challenge: Option<String>,
    pub pkce_code_challenge_method: Option<String>,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRefreshTokenData {
    pub id: i64,
    pub oauth_grant_id: Option<i64>,
    pub app_slug: String,
    pub replaced_by_token_id: Option<i64>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeAccessTokenData {
    pub oauth_grant_id: Option<i64>,
    pub app_slug: String,
    pub oauth_client_id: i64,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub revoked_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeIntrospectionData {
    pub active: bool,
    pub oauth_grant_id: Option<i64>,
    pub client_id: String,
    pub app_slug: String,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayOAuthAccessTokenData {
    pub deployment_id: i64,
    pub oauth_grant_id: Option<i64>,
    pub oauth_client_id: i64,
    pub client_id: String,
    pub app_slug: String,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub rate_limits: Vec<RateLimit>,
    pub rate_limit_scheme_slug: Option<String>,
    pub scope_definitions: Vec<OAuthScopeDefinition>,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeOAuthGrantResolution {
    pub active_grant_id: Option<i64>,
    pub active: bool,
    pub revoked: bool,
}

pub struct ResolveOAuthAppByFqdnQuery {
    pub fqdn: String,
}

impl ResolveOAuthAppByFqdnQuery {
    pub fn new(fqdn: String) -> Self {
        Self { fqdn }
    }
}

impl Query for ResolveOAuthAppByFqdnQuery {
    type Output = Option<RuntimeOAuthAppData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query(
            r#"
            SELECT id, deployment_id, slug, fqdn, supported_scopes, scope_definitions, allow_dynamic_client_registration
            FROM oauth_apps
            WHERE fqdn = $1
              AND is_active = TRUE
            "#,
        )
        .bind(&self.fqdn)
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| RuntimeOAuthAppData {
            id: r.get("id"),
            deployment_id: r.get("deployment_id"),
            slug: r.get("slug"),
            fqdn: r.get("fqdn"),
            supported_scopes: serde_json::from_value(r.get("supported_scopes")).unwrap_or_default(),
            scope_definitions: serde_json::from_value(r.get("scope_definitions"))
                .unwrap_or_default(),
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
}

impl Query for GetRuntimeDeploymentHostsByIdQuery {
    type Output = Option<RuntimeDeploymentHostsData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query(
            r#"
            SELECT backend_host, frontend_host
            FROM deployments
            WHERE id = $1
              AND deleted_at IS NULL
            "#,
        )
        .bind(self.deployment_id)
        .fetch_optional(&app_state.db_pool)
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
}

impl Query for ResolveApiAuthAppSlugByApiKeyHashQuery {
    type Output = Option<String>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
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
}

impl Query for GetRuntimeApiAuthAppSlugByUserIdQuery {
    type Output = Option<String>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
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
}

impl Query for GetRuntimeOAuthClientByClientIdQuery {
    type Output = Option<RuntimeOAuthClientData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
                created_at
            FROM oauth_clients
            WHERE oauth_app_id = $1
              AND client_id = $2
            "#,
        )
        .bind(self.oauth_app_id)
        .bind(&self.client_id)
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| RuntimeOAuthClientData {
            id: r.get("id"),
            oauth_app_id: r.get("oauth_app_id"),
            client_id: r.get("client_id"),
            client_secret_hash: r.get("client_secret_hash"),
            client_secret_encrypted: r.get("client_secret_encrypted"),
            registration_access_token_hash: r.get("registration_access_token_hash"),
            client_auth_method: r.get("client_auth_method"),
            grant_types: serde_json::from_value(r.get("grant_types")).unwrap_or_default(),
            redirect_uris: serde_json::from_value(r.get("redirect_uris")).unwrap_or_default(),
            token_endpoint_auth_signing_alg: r.get("token_endpoint_auth_signing_alg"),
            jwks: r
                .get::<Option<serde_json::Value>, _>("jwks")
                .and_then(|v| serde_json::from_value(v).ok()),
            client_name: r.get("client_name"),
            client_uri: r.get("client_uri"),
            logo_uri: r.get("logo_uri"),
            tos_uri: r.get("tos_uri"),
            policy_uri: r.get("policy_uri"),
            contacts: r
                .get::<Option<serde_json::Value>, _>("contacts")
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            software_id: r.get("software_id"),
            software_version: r.get("software_version"),
            is_active: r.get("is_active"),
            created_at: r.get("created_at"),
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
}

impl Query for ListActiveRuntimeOAuthGrantsQuery {
    type Output = Vec<RuntimeOAuthGrantData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = sqlx::query(
            r#"
            SELECT scopes, resource
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
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| RuntimeOAuthGrantData {
                scopes: serde_json::from_value(r.get("scopes")).unwrap_or_default(),
                resource: r.get("resource"),
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
}

impl Query for GetRuntimeAuthorizationCodeForExchangeQuery {
    type Output = Option<RuntimeAuthorizationCodeData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                oauth_grant_id,
                app_slug,
                redirect_uri,
                pkce_code_challenge,
                pkce_code_challenge_method,
                scopes,
                resource
            FROM oauth_authorization_codes
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND code_hash = $3
              AND consumed_at IS NULL
              AND expires_at > NOW()
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(&self.code_hash)
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| RuntimeAuthorizationCodeData {
            id: r.get("id"),
            oauth_grant_id: r.get("oauth_grant_id"),
            app_slug: r.get("app_slug"),
            redirect_uri: r.get("redirect_uri"),
            pkce_code_challenge: r.get("pkce_code_challenge"),
            pkce_code_challenge_method: r.get("pkce_code_challenge_method"),
            scopes: serde_json::from_value(r.get("scopes")).unwrap_or_default(),
            resource: r.get("resource"),
        }))
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
}

impl Query for GetRuntimeRefreshTokenForExchangeQuery {
    type Output = Option<RuntimeRefreshTokenData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query(
            r#"
            SELECT
                id,
                oauth_grant_id,
                app_slug,
                replaced_by_token_id,
                revoked_at,
                expires_at,
                scopes,
                resource
            FROM oauth_refresh_tokens
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND token_hash = $3
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.oauth_client_id)
        .bind(&self.token_hash)
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| RuntimeRefreshTokenData {
            id: r.get("id"),
            oauth_grant_id: r.get("oauth_grant_id"),
            app_slug: r.get("app_slug"),
            replaced_by_token_id: r.get("replaced_by_token_id"),
            revoked_at: r.get("revoked_at"),
            expires_at: r.get("expires_at"),
            scopes: serde_json::from_value(r.get("scopes")).unwrap_or_default(),
            resource: r.get("resource"),
        }))
    }
}

pub struct ResolveRuntimeOAuthGrantQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub grant_id: Option<i64>,
    pub app_slug: Option<String>,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
}

impl ResolveRuntimeOAuthGrantQuery {
    pub fn by_grant_id(deployment_id: i64, oauth_client_id: i64, grant_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
            grant_id: Some(grant_id),
            app_slug: None,
            scopes: Vec::new(),
            resource: None,
        }
    }

    pub fn by_scope_match(
        deployment_id: i64,
        oauth_client_id: i64,
        app_slug: String,
        scopes: Vec<String>,
        resource: Option<String>,
    ) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
            grant_id: None,
            app_slug: Some(app_slug),
            scopes,
            resource,
        }
    }
}

impl Query for ResolveRuntimeOAuthGrantQuery {
    type Output = RuntimeOAuthGrantResolution;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
                    AND ($5::text IS NULL OR resource = $5)
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
            self.resource,
            scopes_json
        )
        .fetch_one(&app_state.db_pool)
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
}

impl Query for GetRuntimeApiAuthUserIdByAppSlugQuery {
    type Output = Option<i64>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
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
}

impl Query for ValidateRuntimeResourceEntitlementQuery {
    type Output = bool;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let required_permissions: Vec<String> = self
            .required_permissions
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();

        if let Some(id) = self.resource.strip_prefix("urn:wacht:user:") {
            let resource_user_id = id.parse::<i64>().ok().unwrap_or_default();
            return Ok(resource_user_id > 0
                && resource_user_id == self.user_id
                && required_permissions.is_empty());
        }

        if let Some(id) = self.resource.strip_prefix("urn:wacht:organization:") {
            let org_id = id.parse::<i64>().ok().unwrap_or_default();
            if org_id <= 0 {
                return Ok(false);
            }

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
            .fetch_one(&app_state.db_pool)
            .await?;
            return Ok(row.get("entitled"));
        }

        if let Some(id) = self.resource.strip_prefix("urn:wacht:workspace:") {
            let workspace_id = id.parse::<i64>().ok().unwrap_or_default();
            if workspace_id <= 0 {
                return Ok(false);
            }

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
            .fetch_one(&app_state.db_pool)
            .await?;
            return Ok(row.get("entitled"));
        }

        Ok(false)
    }
}

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
}

impl Query for GetRuntimeAccessTokenByHashQuery {
    type Output = Option<RuntimeAccessTokenData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query(
            r#"
            SELECT
                t.oauth_grant_id,
                t.app_slug,
                t.oauth_client_id,
                c.client_id,
                t.scopes,
                t.resource,
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
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| RuntimeAccessTokenData {
            oauth_grant_id: r.get("oauth_grant_id"),
            app_slug: r.get("app_slug"),
            oauth_client_id: r.get("oauth_client_id"),
            client_id: r.get("client_id"),
            scopes: serde_json::from_value(r.get("scopes")).unwrap_or_default(),
            resource: r.get("resource"),
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
}

impl Query for GetRuntimeIntrospectionDataQuery {
    type Output = Option<RuntimeIntrospectionData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
                            WHEN tr.resource LIKE 'urn:wacht:organization:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'organization_permission', '')), '')
                            WHEN tr.resource LIKE 'urn:wacht:workspace:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'workspace_permission', '')), '')
                            ELSE NULL
                        END
                    )
                ) AS perm(category, permission) ON TRUE
                WHERE (def.scope_def->>'scope') IN (SELECT jsonb_array_elements_text(tr.scopes))
                  AND (
                      (tr.resource LIKE 'urn:wacht:organization:%' AND perm.category = 'organization') OR
                      (tr.resource LIKE 'urn:wacht:workspace:%' AND perm.category = 'workspace')
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
                tr.expires_at,
                (
                    tr.revoked_at IS NULL
                    AND tr.expires_at > NOW()
                    AND COALESCE(gs.valid, FALSE)
                    AND CASE
                        WHEN tr.resource IS NULL THEN TRUE
                        WHEN split_part(tr.resource, ':', 1) = 'urn'
                          AND split_part(tr.resource, ':', 2) = 'wacht'
                          AND split_part(tr.resource, ':', 3) = 'user'
                          AND split_part(tr.resource, ':', 4) <> ''
                          AND split_part(tr.resource, ':', 4) !~ '[^0-9]'
                        THEN
                            EXISTS (
                                SELECT 1
                                FROM api_auth_user au
                                WHERE au.user_id = split_part(tr.resource, ':', 4)::bigint
                                  AND cardinality((SELECT permissions FROM required_permission_array)) = 0
                            )
                        WHEN split_part(tr.resource, ':', 1) = 'urn'
                          AND split_part(tr.resource, ':', 2) = 'wacht'
                          AND split_part(tr.resource, ':', 3) = 'organization'
                          AND split_part(tr.resource, ':', 4) <> ''
                          AND split_part(tr.resource, ':', 4) !~ '[^0-9]'
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
                                       AND om.organization_id = split_part(tr.resource, ':', 4)::bigint
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
                        WHEN split_part(tr.resource, ':', 1) = 'urn'
                          AND split_part(tr.resource, ':', 2) = 'wacht'
                          AND split_part(tr.resource, ':', 3) = 'workspace'
                          AND split_part(tr.resource, ':', 4) <> ''
                          AND split_part(tr.resource, ':', 4) !~ '[^0-9]'
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
                                       AND wm.workspace_id = split_part(tr.resource, ':', 4)::bigint
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
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| RuntimeIntrospectionData {
            active: r.active.unwrap_or(false),
            oauth_grant_id: r.oauth_grant_id,
            client_id: r.client_id,
            app_slug: r.app_slug,
            scopes: serde_json::from_value(r.scopes).unwrap_or_default(),
            resource: r.resource,
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
}

impl Query for GetGatewayOAuthAccessTokenByHashQuery {
    type Output = Option<GatewayOAuthAccessTokenData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            r#"
            WITH token_row AS (
                SELECT
                    t.deployment_id,
                    t.oauth_grant_id,
                    t.oauth_client_id,
                    c.client_id,
                    t.app_slug,
                    t.scopes,
                    t.resource,
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
                            WHEN tr.resource LIKE 'urn:wacht:organization:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'organization_permission', '')), '')
                            WHEN tr.resource LIKE 'urn:wacht:workspace:%'
                                THEN NULLIF(TRIM(COALESCE(def.scope_def->>'workspace_permission', '')), '')
                            ELSE NULL
                        END
                    )
                ) AS perm(category, permission) ON TRUE
                WHERE (def.scope_def->>'scope') IN (SELECT jsonb_array_elements_text(tr.scopes))
                  AND (
                      (tr.resource LIKE 'urn:wacht:organization:%' AND perm.category = 'organization') OR
                      (tr.resource LIKE 'urn:wacht:workspace:%' AND perm.category = 'workspace')
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
                tr.scopes as "scopes!: serde_json::Value",
                tr.resource,
                tr.expires_at,
                tr.rate_limit_scheme_slug,
                tr.scope_definitions as "scope_definitions!: serde_json::Value",
                (
                    tr.api_auth_app_is_active IS TRUE
                    AND tr.revoked_at IS NULL
                    AND tr.expires_at > NOW()
                    AND COALESCE(gs.valid, FALSE)
                    AND CASE
                        WHEN tr.resource IS NULL THEN TRUE
                        WHEN split_part(tr.resource, ':', 1) = 'urn'
                          AND split_part(tr.resource, ':', 2) = 'wacht'
                          AND split_part(tr.resource, ':', 3) = 'user'
                          AND split_part(tr.resource, ':', 4) <> ''
                          AND split_part(tr.resource, ':', 4) !~ '[^0-9]'
                        THEN
                            tr.user_id = split_part(tr.resource, ':', 4)::bigint
                            AND cardinality((SELECT permissions FROM required_permission_array)) = 0
                        WHEN split_part(tr.resource, ':', 1) = 'urn'
                          AND split_part(tr.resource, ':', 2) = 'wacht'
                          AND split_part(tr.resource, ':', 3) = 'organization'
                          AND split_part(tr.resource, ':', 4) <> ''
                          AND split_part(tr.resource, ':', 4) !~ '[^0-9]'
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
                                      AND om.organization_id = split_part(tr.resource, ':', 4)::bigint
                                      AND om.deleted_at IS NULL
                                ) perms
                                WHERE perms.membership_count > 0
                                  AND perms.permissions @> (SELECT permissions FROM required_permission_array)
                            )
                        WHEN split_part(tr.resource, ':', 1) = 'urn'
                          AND split_part(tr.resource, ':', 2) = 'wacht'
                          AND split_part(tr.resource, ':', 3) = 'workspace'
                          AND split_part(tr.resource, ':', 4) <> ''
                          AND split_part(tr.resource, ':', 4) !~ '[^0-9]'
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
                                      AND wm.workspace_id = split_part(tr.resource, ':', 4)::bigint
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
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(row.map(|r| GatewayOAuthAccessTokenData {
            deployment_id: r.deployment_id,
            oauth_grant_id: r.oauth_grant_id,
            oauth_client_id: r.oauth_client_id,
            client_id: r.client_id,
            app_slug: r.app_slug,
            scopes: serde_json::from_value(r.scopes).unwrap_or_default(),
            resource: r.resource,
            expires_at: r.expires_at,
            rate_limits: vec![],
            rate_limit_scheme_slug: r.rate_limit_scheme_slug,
            scope_definitions: serde_json::from_value(r.scope_definitions).unwrap_or_default(),
            active: r.active,
        }))
    }
}
