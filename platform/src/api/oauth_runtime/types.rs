use models::api_key::OAuthScopeDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(crate) struct ClientAssertionClaims {
    pub(crate) iss: Option<String>,
    pub(crate) sub: Option<String>,
    pub(crate) aud: Option<serde_json::Value>,
    pub(crate) exp: Option<i64>,
    pub(crate) iat: Option<i64>,
    pub(crate) nbf: Option<i64>,
    pub(crate) jti: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OAuthConsentRequestTokenClaims {
    pub(crate) exp: i64,
    pub(crate) iat: i64,
    pub(crate) jti: String,
    pub(crate) deployment_id: i64,
    pub(crate) oauth_client_id: i64,
    pub(crate) client_id: String,
    pub(crate) redirect_uri: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) resource: Option<String>,
    pub(crate) state: Option<String>,
    pub(crate) code_challenge: Option<String>,
    pub(crate) code_challenge_method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OAuthConsentHandoffPayload {
    pub(crate) request_token: String,
    pub(crate) issuer: String,
    pub(crate) deployment_id: i64,
    pub(crate) oauth_client_id: i64,
    pub(crate) client_id: String,
    pub(crate) redirect_uri: String,
    pub(crate) scopes: Vec<String>,
    pub(crate) scope_definitions: Vec<OAuthScopeDefinition>,
    pub(crate) resource: Option<String>,
    pub(crate) resource_options: Vec<String>,
    pub(crate) state: Option<String>,
    pub(crate) expires_at: i64,
    pub(crate) client_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrantValidationResult {
    Active,
    Revoked,
    MissingOrInsufficient,
}
