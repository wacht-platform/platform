use models::api_key::OAuthScopeDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub(super) struct ClientAssertionClaims {
    pub(super) iss: Option<String>,
    pub(super) sub: Option<String>,
    pub(super) aud: Option<serde_json::Value>,
    pub(super) exp: Option<i64>,
    pub(super) iat: Option<i64>,
    pub(super) nbf: Option<i64>,
    pub(super) jti: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct OAuthConsentRequestTokenClaims {
    pub(super) exp: i64,
    pub(super) iat: i64,
    pub(super) jti: String,
    pub(super) deployment_id: i64,
    pub(super) oauth_client_id: i64,
    pub(super) client_id: String,
    pub(super) redirect_uri: String,
    pub(super) scopes: Vec<String>,
    pub(super) resource: Option<String>,
    pub(super) state: Option<String>,
    pub(super) code_challenge: Option<String>,
    pub(super) code_challenge_method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OAuthConsentHandoffPayload {
    pub(super) request_token: String,
    pub(super) issuer: String,
    pub(super) deployment_id: i64,
    pub(super) oauth_client_id: i64,
    pub(super) client_id: String,
    pub(super) redirect_uri: String,
    pub(super) scopes: Vec<String>,
    pub(super) scope_definitions: Vec<OAuthScopeDefinition>,
    pub(super) resource: Option<String>,
    pub(super) resource_options: Vec<String>,
    pub(super) state: Option<String>,
    pub(super) expires_at: i64,
    pub(super) client_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GrantValidationResult {
    Active,
    Revoked,
    MissingOrInsufficient,
}
