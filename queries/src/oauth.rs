use super::Query;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use models::api_key::{JwksDocument, OAuthScopeDefinition};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthAppData {
    pub id: i64,
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub fqdn: String,
    pub supported_scopes: serde_json::Value,
    pub scope_definitions: serde_json::Value,
    pub allow_dynamic_client_registration: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthAppData {
    pub fn supported_scopes_vec(&self) -> Vec<String> {
        serde_json::from_value(self.supported_scopes.clone()).unwrap_or_default()
    }

    pub fn scope_definitions_vec(&self) -> Vec<OAuthScopeDefinition> {
        serde_json::from_value(self.scope_definitions.clone()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientData {
    pub id: i64,
    pub deployment_id: i64,
    pub oauth_app_id: i64,
    pub client_id: String,
    pub client_auth_method: String,
    pub grant_types: serde_json::Value,
    pub redirect_uris: serde_json::Value,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: serde_json::Value,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub pkce_required: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthClientData {
    pub fn grant_types_vec(&self) -> Vec<String> {
        serde_json::from_value(self.grant_types.clone()).unwrap_or_default()
    }

    pub fn redirect_uris_vec(&self) -> Vec<String> {
        serde_json::from_value(self.redirect_uris.clone()).unwrap_or_default()
    }

    pub fn contacts_vec(&self) -> Vec<String> {
        serde_json::from_value(self.contacts.clone()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientGrantData {
    pub id: i64,
    pub deployment_id: i64,
    pub api_auth_app_slug: String,
    pub oauth_client_id: i64,
    pub resource: String,
    pub scopes: serde_json::Value,
    pub status: String,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub granted_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl OAuthClientGrantData {
    pub fn scopes_vec(&self) -> Vec<String> {
        serde_json::from_value(self.scopes.clone()).unwrap_or_default()
    }
}

pub struct ListOAuthAppsByDeploymentQuery {
    pub deployment_id: i64,
}

impl ListOAuthAppsByDeploymentQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<OAuthAppData>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                deployment_id,
                slug,
                name,
                description,
                logo_url,
                fqdn,
                supported_scopes as "supported_scopes: serde_json::Value",
                scope_definitions as "scope_definitions: serde_json::Value",
                allow_dynamic_client_registration,
                is_active,
                created_at,
                updated_at
            FROM oauth_apps
            WHERE deployment_id = $1
            ORDER BY created_at DESC
            "#,
            self.deployment_id
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OAuthAppData {
                id: r.id,
                deployment_id: r.deployment_id,
                slug: r.slug,
                name: r.name,
                description: r.description,
                logo_url: r.logo_url,
                fqdn: r.fqdn,
                supported_scopes: r.supported_scopes,
                scope_definitions: r.scope_definitions,
                allow_dynamic_client_registration: r.allow_dynamic_client_registration,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}

impl Query for ListOAuthAppsByDeploymentQuery {
    type Output = Vec<OAuthAppData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

pub struct GetOAuthAppBySlugQuery {
    pub deployment_id: i64,
    pub oauth_app_slug: String,
}

impl GetOAuthAppBySlugQuery {
    pub fn new(deployment_id: i64, oauth_app_slug: String) -> Self {
        Self {
            deployment_id,
            oauth_app_slug,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Option<OAuthAppData>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                deployment_id,
                slug,
                name,
                description,
                logo_url,
                fqdn,
                supported_scopes as "supported_scopes: serde_json::Value",
                scope_definitions as "scope_definitions: serde_json::Value",
                allow_dynamic_client_registration,
                is_active,
                created_at,
                updated_at
            FROM oauth_apps
            WHERE deployment_id = $1
              AND slug = $2
            "#,
            self.deployment_id,
            self.oauth_app_slug
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(row.map(|r| OAuthAppData {
            id: r.id,
            deployment_id: r.deployment_id,
            slug: r.slug,
            name: r.name,
            description: r.description,
            logo_url: r.logo_url,
            fqdn: r.fqdn,
            supported_scopes: r.supported_scopes,
            scope_definitions: r.scope_definitions,
            allow_dynamic_client_registration: r.allow_dynamic_client_registration,
            is_active: r.is_active,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }))
    }
}

impl Query for GetOAuthAppBySlugQuery {
    type Output = Option<OAuthAppData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

pub struct ListOAuthClientsByOAuthAppQuery {
    pub deployment_id: i64,
    pub oauth_app_id: i64,
}

impl ListOAuthClientsByOAuthAppQuery {
    pub fn new(deployment_id: i64, oauth_app_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_app_id,
        }
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Vec<OAuthClientData>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                deployment_id,
                oauth_app_id,
                client_id,
                client_auth_method,
                grant_types as "grant_types: serde_json::Value",
                redirect_uris as "redirect_uris: serde_json::Value",
                token_endpoint_auth_signing_alg,
                jwks_uri,
                jwks as "jwks: sqlx::types::Json<models::api_key::JwksDocument>",
                client_name,
                client_uri,
                logo_uri,
                tos_uri,
                policy_uri,
                contacts as "contacts: serde_json::Value",
                software_id,
                software_version,
                pkce_required,
                is_active,
                created_at,
                updated_at
            FROM oauth_clients
            WHERE deployment_id = $1
              AND oauth_app_id = $2
            ORDER BY created_at DESC
            "#,
            self.deployment_id,
            self.oauth_app_id
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let jwks = r.jwks.map(|j| j.0);
                let public_key_pem = jwks.as_ref().and_then(JwksDocument::public_key_pem);
                OAuthClientData {
                    id: r.id,
                    deployment_id: r.deployment_id,
                    oauth_app_id: r.oauth_app_id,
                    client_id: r.client_id,
                    client_auth_method: r.client_auth_method,
                    grant_types: r.grant_types,
                    redirect_uris: r.redirect_uris,
                    token_endpoint_auth_signing_alg: r.token_endpoint_auth_signing_alg,
                    jwks_uri: r.jwks_uri,
                    jwks,
                    public_key_pem,
                    client_name: r.client_name,
                    client_uri: r.client_uri,
                    logo_uri: r.logo_uri,
                    tos_uri: r.tos_uri,
                    policy_uri: r.policy_uri,
                    contacts: r.contacts,
                    software_id: r.software_id,
                    software_version: r.software_version,
                    pkce_required: r.pkce_required,
                    is_active: r.is_active,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                }
            })
            .collect())
    }
}

impl Query for ListOAuthClientsByOAuthAppQuery {
    type Output = Vec<OAuthClientData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

pub struct GetOAuthClientByIdQuery {
    pub deployment_id: i64,
    pub oauth_app_id: i64,
    pub oauth_client_id: i64,
}

impl GetOAuthClientByIdQuery {
    pub fn new(deployment_id: i64, oauth_app_id: i64, oauth_client_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_app_id,
            oauth_client_id,
        }
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Option<OAuthClientData>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            r#"
            SELECT
                id,
                deployment_id,
                oauth_app_id,
                client_id,
                client_auth_method,
                grant_types as "grant_types: serde_json::Value",
                redirect_uris as "redirect_uris: serde_json::Value",
                token_endpoint_auth_signing_alg,
                jwks_uri,
                jwks as "jwks: sqlx::types::Json<models::api_key::JwksDocument>",
                client_name,
                client_uri,
                logo_uri,
                tos_uri,
                policy_uri,
                contacts as "contacts: serde_json::Value",
                software_id,
                software_version,
                pkce_required,
                is_active,
                created_at,
                updated_at
            FROM oauth_clients
            WHERE deployment_id = $1
              AND oauth_app_id = $2
              AND id = $3
            "#,
            self.deployment_id,
            self.oauth_app_id,
            self.oauth_client_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(row.map(|r| {
            let jwks = r.jwks.map(|j| j.0);
            let public_key_pem = jwks.as_ref().and_then(JwksDocument::public_key_pem);
            OAuthClientData {
                id: r.id,
                deployment_id: r.deployment_id,
                oauth_app_id: r.oauth_app_id,
                client_id: r.client_id,
                client_auth_method: r.client_auth_method,
                grant_types: r.grant_types,
                redirect_uris: r.redirect_uris,
                token_endpoint_auth_signing_alg: r.token_endpoint_auth_signing_alg,
                jwks_uri: r.jwks_uri,
                jwks,
                public_key_pem,
                client_name: r.client_name,
                client_uri: r.client_uri,
                logo_uri: r.logo_uri,
                tos_uri: r.tos_uri,
                policy_uri: r.policy_uri,
                contacts: r.contacts,
                software_id: r.software_id,
                software_version: r.software_version,
                pkce_required: r.pkce_required,
                is_active: r.is_active,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        }))
    }
}

impl Query for GetOAuthClientByIdQuery {
    type Output = Option<OAuthClientData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

pub struct ListOAuthGrantsByClientQuery {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
}

impl ListOAuthGrantsByClientQuery {
    pub fn new(deployment_id: i64, oauth_client_id: i64) -> Self {
        Self {
            deployment_id,
            oauth_client_id,
        }
    }

    pub async fn execute_with<'a, A>(
        &self,
        acquirer: A,
    ) -> Result<Vec<OAuthClientGrantData>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rows = sqlx::query!(
            r#"
            SELECT
                g.id,
                g.deployment_id,
                g.app_slug as api_auth_app_slug,
                g.oauth_client_id,
                g.resource,
                g.scopes as "scopes: serde_json::Value",
                g.status,
                g.granted_at,
                g.expires_at,
                g.revoked_at,
                g.granted_by_user_id,
                g.created_at,
                g.updated_at
            FROM oauth_client_grants g
            WHERE g.deployment_id = $1
              AND g.oauth_client_id = $2
            ORDER BY g.created_at DESC
            "#,
            self.deployment_id,
            self.oauth_client_id
        )
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| OAuthClientGrantData {
                id: r.id,
                deployment_id: r.deployment_id,
                api_auth_app_slug: r.api_auth_app_slug,
                oauth_client_id: r.oauth_client_id,
                resource: r.resource,
                scopes: r.scopes,
                status: r.status,
                granted_at: r.granted_at,
                expires_at: r.expires_at,
                revoked_at: r.revoked_at,
                granted_by_user_id: r.granted_by_user_id,
                created_at: r.created_at,
                updated_at: r.updated_at,
            })
            .collect())
    }
}

impl Query for ListOAuthGrantsByClientQuery {
    type Output = Vec<OAuthClientGrantData>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}
