use models::api_key::JwksDocument;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Clone)]
pub struct OAuthAuthorizeRequest {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub redirect_uri: Option<String>,
    pub scope: Option<String>,
    pub state: Option<String>,
    pub resource: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub response_mode: Option<String>,
    // OIDC params (only meaningful when `openid` is in scope).
    pub nonce: Option<String>,
    pub prompt: Option<String>,
    pub max_age: Option<i64>,
    pub id_token_hint: Option<String>,
    pub login_hint: Option<String>,
    pub ui_locales: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthAuthorizeResponse {
    pub code: String,
    pub state: Option<String>,
    pub expires_in: i64,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize)]
pub struct OAuthAuthorizeInitiatedResponse {
    pub consent_url: String,
    pub expires_in: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct OAuthTokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub scope: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    pub refresh_token: String,
    pub scope: String,
    /// Signed OIDC id_token. Present only when the original auth request
    /// included the `openid` scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthRevokeRequest {
    pub token: String,
    pub token_type_hint: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthRevokeResponse {
    pub revoked: bool,
}

#[derive(Debug, Serialize)]
pub struct OAuthServerMetadataResponse {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: String,
    pub introspection_endpoint: String,
    pub registration_endpoint: String,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub scopes_supported: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthProtectedResourceMetadataResponse {
    pub resource: String,
    pub authorization_servers: Vec<String>,
    pub bearer_methods_supported: Vec<String>,
    pub scopes_supported: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthIntrospectRequest {
    pub token: String,
    pub token_type_hint: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub client_assertion_type: Option<String>,
    pub client_assertion: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthIntrospectResponse {
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted_resource: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthDynamicClientRegistrationRequest {
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Option<Vec<String>>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub token_endpoint_auth_method: Option<String>,
    #[serde(default)]
    pub grant_types: Vec<String>,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
    /// OIDC: RP-initiated logout allowlist.
    #[serde(default)]
    pub post_logout_redirect_uris: Vec<String>,
    /// OIDC: id_token signing alg. The internal field name is
    /// `id_token_signing_alg`; OIDC Dynamic Client Registration §2 names this
    /// `id_token_signed_response_alg`. Both names deserialize to the same
    /// field so spec-conformant clients work out of the box.
    #[serde(alias = "id_token_signed_response_alg")]
    pub id_token_signing_alg: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct OAuthDynamicClientRegistrationResponse {
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tos_uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_uri: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub contacts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub software_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub client_id_issued_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_expires_at: Option<i64>,
    pub token_endpoint_auth_method: String,
    pub grant_types: Vec<String>,
    pub redirect_uris: Vec<String>,
    pub registration_client_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_access_token: Option<String>,
    /// OIDC RP-initiated logout allowlist registered on the client.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub post_logout_redirect_uris: Vec<String>,
    /// id_token signing algorithm. Returned under the OIDC spec field name
    /// `id_token_signed_response_alg`; deserializers ignore the legacy
    /// `id_token_signing_alg` alias on writes.
    #[serde(rename = "id_token_signed_response_alg")]
    pub id_token_signed_response_alg: String,
}

#[derive(Debug, Deserialize)]
pub struct OAuthDynamicClientUpdateRequest {
    pub client_name: Option<String>,
    pub client_uri: Option<String>,
    pub logo_uri: Option<String>,
    pub tos_uri: Option<String>,
    pub policy_uri: Option<String>,
    pub contacts: Option<Vec<String>>,
    pub software_id: Option<String>,
    pub software_version: Option<String>,
    pub token_endpoint_auth_method: Option<String>,
    pub grant_types: Option<Vec<String>>,
    pub redirect_uris: Option<Vec<String>>,
    pub token_endpoint_auth_signing_alg: Option<String>,
    pub jwks_uri: Option<String>,
    pub jwks: Option<JwksDocument>,
    pub public_key_pem: Option<String>,
    /// OIDC: replaces the entire `post_logout_redirect_uris` allowlist when present.
    pub post_logout_redirect_uris: Option<Vec<String>>,
    /// OIDC: id_token signing alg. The spec field name in OIDC Dynamic Client
    /// Registration §2 is `id_token_signed_response_alg`; alias it so both
    /// names work.
    #[serde(alias = "id_token_signed_response_alg")]
    pub id_token_signing_alg: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthConsentSubmitRequest {
    pub request_token: String,
    pub action: String,
    pub resource: Option<String>,
    pub granted_resource: Option<String>,
    pub scope: Option<String>,
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub user_id: i64,
    /// OIDC: the Wacht session that authorized this consent. frontend-api reads
    /// it from the active session cookie and forwards it here, so we can bind
    /// the issued tokens to the same session that signed off on consent.
    #[serde(
        default,
        with = "models::utils::serde::i64_as_string_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct OAuthRegisterPathParams {
    pub client_id: String,
}

// ============================================================================
// OIDC extension
// ============================================================================

/// `.well-known/openid-configuration` response. Superset of
/// `OAuthServerMetadataResponse` with OIDC-specific fields.
#[derive(Debug, Serialize)]
pub struct OpenIdConfigurationResponse {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub userinfo_endpoint: String,
    pub end_session_endpoint: String,
    pub jwks_uri: String,
    pub registration_endpoint: String,
    pub revocation_endpoint: String,
    pub introspection_endpoint: String,
    pub response_types_supported: Vec<String>,
    pub grant_types_supported: Vec<String>,
    pub subject_types_supported: Vec<String>,
    pub id_token_signing_alg_values_supported: Vec<String>,
    pub scopes_supported: Vec<String>,
    pub claims_supported: Vec<String>,
    pub token_endpoint_auth_methods_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
    pub response_modes_supported: Vec<String>,
    pub request_parameter_supported: bool,
    pub request_uri_parameter_supported: bool,
}

/// One key in the JWKS document.
#[derive(Debug, Serialize)]
pub struct JwkKey {
    pub kty: String,
    #[serde(rename = "use")]
    pub key_use: String,
    pub alg: String,
    pub kid: String,
    /// Modulus (base64url, no padding) for RSA keys.
    pub n: String,
    /// Exponent (base64url, no padding) for RSA keys.
    pub e: String,
}

#[derive(Debug, Serialize)]
pub struct JwksResponse {
    pub keys: Vec<JwkKey>,
}

/// OIDC userinfo claims. Fields populated based on the access token's scope.
#[derive(Debug, Serialize, Default)]
pub struct UserInfoResponse {
    pub sub: String,
    // profile scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
    // email scope
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
}

/// id_token claims. Built when `openid` scope was requested.
#[derive(Debug, Serialize)]
pub struct IdTokenClaims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub exp: i64,
    pub iat: i64,
    pub auth_time: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// SHA-256 hash of the access_token, first 128 bits, base64url —
    /// binds the id_token to its companion access_token (defends against
    /// token substitution).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_hash: Option<String>,
    /// OIDC session identifier. Required for RP-initiated logout to locate
    /// and revoke the Wacht session the id_token was issued against.
    /// Serialized as a string because session ids are i64 and JWTs leak
    /// precision for numbers above 2^53.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sid: Option<String>,
    // profile claims (gated on `profile` scope)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub given_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_username: Option<String>,
    // email claims (gated on `email` scope)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email_verified: Option<bool>,
}

/// Admin view of a signing key. Private material is intentionally excluded —
/// operators never need to see it, and not serializing it keeps it out of any
/// logs/metrics that capture API responses.
#[derive(Debug, Serialize)]
pub struct OAuthAppSigningKeySummary {
    pub kid: String,
    pub algorithm: String,
    pub status: String,
    pub public_key_pem: String,
}

#[derive(Debug, Serialize)]
pub struct OAuthAppSigningKeysListResponse {
    pub keys: Vec<OAuthAppSigningKeySummary>,
}

/// Response from a rotate. Returns the new active key + the kid of the one
/// that was retired (None if it's the first key).
#[derive(Debug, Serialize)]
pub struct OAuthAppSigningKeyRotatedResponse {
    pub new: OAuthAppSigningKeySummary,
}

/// RP-initiated logout (`end_session_endpoint`) request.
#[derive(Debug, Deserialize)]
pub struct OAuthLogoutRequest {
    /// Previous id_token — proves which user + client is logging out.
    pub id_token_hint: Option<String>,
    /// Where to send the user after logout. Must be in the client's
    /// `post_logout_redirect_uris` allowlist.
    pub post_logout_redirect_uri: Option<String>,
    /// Client id (used to validate redirect URI when id_token_hint absent).
    pub client_id: Option<String>,
    /// CSRF protection on the redirect-back.
    pub state: Option<String>,
    pub logout_hint: Option<String>,
    pub ui_locales: Option<String>,
}
