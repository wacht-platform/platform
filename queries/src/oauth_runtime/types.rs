use models::api_key::{JwksDocument, OAuthScopeDefinition, RateLimit};
use serde::{Deserialize, Serialize};

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
    pub granted_resource: Option<String>,
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
    pub granted_resource: Option<String>,
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
    pub granted_resource: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeAccessTokenData {
    pub oauth_grant_id: Option<i64>,
    pub app_slug: String,
    pub oauth_client_id: i64,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
    pub granted_resource: Option<String>,
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
    pub granted_resource: Option<String>,
    pub issued_at: chrono::DateTime<chrono::Utc>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayOAuthAccessTokenData {
    pub deployment_id: i64,
    pub oauth_grant_id: Option<i64>,
    pub oauth_client_id: i64,
    pub client_id: String,
    pub app_slug: String,
    pub oauth_issuer: String,
    pub owner_user_id: Option<i64>,
    pub scopes: Vec<String>,
    pub resource: Option<String>,
    pub granted_resource: Option<String>,
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
