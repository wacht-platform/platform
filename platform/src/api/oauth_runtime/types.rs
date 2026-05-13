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
    /// OIDC nonce from the original /authorize request. Signed here so it
    /// can't be tampered with on the way through consent → code → id_token.
    #[serde(default)]
    pub(crate) nonce: Option<String>,
    #[serde(default)]
    pub(crate) auth_time: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    pub(crate) session_id: Option<i64>,
    /// Space-separated `prompt` values from the original /authorize request.
    /// Frontend honors these (skip-consent / picker); platform enforces
    /// `max_age` and rejects unknown values up front.
    #[serde(default)]
    pub(crate) prompt: Option<String>,
    /// Maximum permitted age in seconds for the End-User authentication.
    /// Enforced in consent_submit against the resolved signin's created_at.
    #[serde(default)]
    pub(crate) max_age: Option<i64>,
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
    /// Forwarded to the consent UI so it can branch on `prompt=none` /
    /// `prompt=select_account` etc.
    #[serde(default)]
    pub(crate) prompt: Option<String>,
    #[serde(default)]
    pub(crate) max_age: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GrantValidationResult {
    Active,
    Revoked,
    MissingOrInsufficient,
}
