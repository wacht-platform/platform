//! OIDC extension endpoints. id_token issuance and OIDC-specific `/authorize`
//! parameters live in sibling `token.rs` / `authorize.rs`.

use axum::http::HeaderMap;
use axum::response::Redirect;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use common::state::AppState;
use dto::json::oauth_runtime::{
    IdTokenClaims, JwkKey, JwksResponse, OAuthLogoutRequest, OpenIdConfigurationResponse,
    UserInfoResponse,
};
use rsa::pkcs8::{DecodePublicKey, EncodePrivateKey, EncodePublicKey, LineEnding};
use rsa::traits::PublicKeyParts;
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::application::response::ApiErrorResponse;

use super::resolve_oauth_app_and_issuer;

use commands::oauth_runtime::{
    CompromiseOAuthAppSigningKey, InsertOAuthAppSigningKey, RotateOAuthAppSigningKey,
};
use queries::oauth_runtime::{
    GetOAuthAppActiveSigningKeyQuery, ListOAuthAppPublishableSigningKeysQuery,
    OAuthAppPublishableKey, OAuthAppSigningKey,
};

/// Return the active signing key for the app, lazy-provisioning one if none
/// exists. Concurrent callers are serialised by the unique partial index on
/// `(oauth_app_id) WHERE status='active'`; losers re-read.
pub(crate) async fn ensure_active_signing_key(
    app_state: &AppState,
    oauth_app_id: i64,
) -> Result<OAuthAppSigningKey, ApiErrorResponse> {
    let writer = app_state.db_router.writer();

    if let Some(key) = GetOAuthAppActiveSigningKeyQuery::new(oauth_app_id)
        .execute_with_db(writer)
        .await
        .map_err(ApiErrorResponse::from)?
    {
        return Ok(key);
    }

    let (id, kid, public_pem, private_pem) =
        generate_rsa_signing_material(app_state, oauth_app_id)?;

    InsertOAuthAppSigningKey {
        id,
        oauth_app_id,
        kid,
        algorithm: "RS256".to_string(),
        public_key_pem: public_pem,
        private_key_pem: private_pem,
    }
    .execute_with_db(writer)
    .await
    .map_err(ApiErrorResponse::from)?;

    GetOAuthAppActiveSigningKeyQuery::new(oauth_app_id)
        .execute_with_db(writer)
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::internal("signing key vanished after insert"))
}

fn generate_rsa_signing_material(
    app_state: &AppState,
    oauth_app_id: i64,
) -> Result<(i64, String, String, String), ApiErrorResponse> {
    let mut rng = rsa::rand_core::OsRng;
    let private_key = RsaPrivateKey::new(&mut rng, 2048)
        .map_err(|e| ApiErrorResponse::internal(format!("RSA key generation failed: {}", e)))?;
    let public_key = RsaPublicKey::from(&private_key);
    let private_pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .map_err(|e| ApiErrorResponse::internal(format!("private PEM encode failed: {}", e)))?
        .to_string();
    let public_pem = public_key
        .to_public_key_pem(LineEnding::LF)
        .map_err(|e| ApiErrorResponse::internal(format!("public PEM encode failed: {}", e)))?;
    let id: i64 = app_state
        .sf
        .next_id()
        .map_err(|e| ApiErrorResponse::internal(format!("id generation failed: {}", e)))?
        as i64;
    let kid = format!("oas-{}-{}", oauth_app_id, id);
    Ok((id, kid, public_pem, private_pem))
}

/// Atomic: retire the current active key (kept in JWKS for grace) and
/// install a fresh one as the new active.
pub async fn rotate_app_signing_key(
    app_state: &AppState,
    oauth_app_id: i64,
) -> Result<OAuthAppPublishableKey, ApiErrorResponse> {
    let (new_id, new_kid, new_public_pem, new_private_pem) =
        generate_rsa_signing_material(app_state, oauth_app_id)?;

    RotateOAuthAppSigningKey {
        oauth_app_id,
        new_id,
        new_kid: new_kid.clone(),
        new_algorithm: "RS256".to_string(),
        new_public_key_pem: new_public_pem.clone(),
        new_private_key_pem: new_private_pem.clone(),
    }
    .execute_with_pool(app_state.db_router.writer())
    .await
    .map_err(ApiErrorResponse::from)?;

    let full = GetOAuthAppActiveSigningKeyQuery::new(oauth_app_id)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::internal("rotated key vanished after insert"))?;
    Ok(full.into())
}

/// Pull a key from JWKS immediately. Any id_token signed by it stops
/// verifying — use only on suspected private-key leak.
pub async fn compromise_app_signing_key(
    app_state: &AppState,
    oauth_app_id: i64,
    kid: String,
) -> Result<(), ApiErrorResponse> {
    let updated = CompromiseOAuthAppSigningKey {
        oauth_app_id,
        kid: kid.clone(),
    }
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(ApiErrorResponse::from)?;

    if !updated {
        return Err(ApiErrorResponse::not_found(format!(
            "no signing key with kid '{}' for this app",
            kid
        )));
    }
    Ok(())
}

pub async fn list_app_signing_keys(
    app_state: &AppState,
    oauth_app_id: i64,
) -> Result<Vec<OAuthAppPublishableKey>, ApiErrorResponse> {
    ListOAuthAppPublishableSigningKeysQuery::new(oauth_app_id)
        .execute_with_db(app_state.db_router.reader(common::db_router::ReadConsistency::Strong))
        .await
        .map_err(ApiErrorResponse::from)
}

// ---------------------------------------------------------------------------
// /.well-known/openid-configuration
pub async fn openid_configuration(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<OpenIdConfigurationResponse, ApiErrorResponse> {
    let (oauth_app, issuer) = resolve_oauth_app_and_issuer(app_state, headers).await?;

    // OIDC standard scopes are always supported on top of the app's catalog.
    let mut scopes = oauth_app.active_scopes();
    for s in ["openid", "profile", "email", "offline_access"] {
        if !scopes.iter().any(|x| x == s) {
            scopes.push(s.to_string());
        }
    }

    Ok(OpenIdConfigurationResponse {
        authorization_endpoint: format!("{}/oauth/authorize", issuer),
        token_endpoint: format!("{}/oauth/token", issuer),
        userinfo_endpoint: format!("{}/oauth/userinfo", issuer),
        end_session_endpoint: format!("{}/oauth/logout", issuer),
        jwks_uri: format!("{}/.well-known/jwks.json", issuer),
        registration_endpoint: format!("{}/oauth/register", issuer),
        revocation_endpoint: format!("{}/oauth/revoke", issuer),
        introspection_endpoint: format!("{}/oauth/introspect", issuer),
        issuer,
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        subject_types_supported: vec!["public".to_string()],
        id_token_signing_alg_values_supported: vec!["RS256".to_string()],
        scopes_supported: scopes,
        claims_supported: vec![
            "sub".to_string(),
            "iss".to_string(),
            "aud".to_string(),
            "exp".to_string(),
            "iat".to_string(),
            "auth_time".to_string(),
            "nonce".to_string(),
            "name".to_string(),
            "given_name".to_string(),
            "family_name".to_string(),
            "picture".to_string(),
            "preferred_username".to_string(),
            "email".to_string(),
            "email_verified".to_string(),
        ],
        token_endpoint_auth_methods_supported: vec![
            "client_secret_basic".to_string(),
            "client_secret_post".to_string(),
            "client_secret_jwt".to_string(),
            "private_key_jwt".to_string(),
            "none".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string()],
        response_modes_supported: vec!["query".to_string()],
        request_parameter_supported: false,
        request_uri_parameter_supported: false,
    })
}

fn jwk_from_publishable(key: &OAuthAppPublishableKey) -> Result<JwkKey, ApiErrorResponse> {
    let pub_key = RsaPublicKey::from_public_key_pem(&key.public_key_pem)
        .map_err(|e| ApiErrorResponse::internal(format!("Invalid public key PEM: {}", e)))?;
    Ok(JwkKey {
        kty: "RSA".to_string(),
        key_use: "sig".to_string(),
        alg: key.algorithm.clone(),
        kid: key.kid.clone(),
        n: URL_SAFE_NO_PAD.encode(pub_key.n().to_bytes_be()),
        e: URL_SAFE_NO_PAD.encode(pub_key.e().to_bytes_be()),
    })
}

pub async fn jwks(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<JwksResponse, ApiErrorResponse> {
    let (oauth_app, _issuer) = resolve_oauth_app_and_issuer(app_state, headers).await?;
    let _active = ensure_active_signing_key(app_state, oauth_app.id).await?;

    let reader = app_state
        .db_router
        .reader(common::db_router::ReadConsistency::Strong);
    let keys = ListOAuthAppPublishableSigningKeysQuery::new(oauth_app.id)
        .execute_with_db(reader)
        .await
        .map_err(ApiErrorResponse::from)?;

    let jwks_keys = keys
        .iter()
        .map(jwk_from_publishable)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(JwksResponse { keys: jwks_keys })
}

/// Bearer-protected-resource error per RFC 6750 §3 / OIDC Core 5.3.3 —
/// surfaced as a `WWW-Authenticate: Bearer …` header.
pub enum UserInfoError {
    MissingToken,
    InvalidToken(&'static str),
    InsufficientScope { required: &'static str, message: &'static str },
    InvalidRequest(&'static str),
    Internal(ApiErrorResponse),
}

impl From<ApiErrorResponse> for UserInfoError {
    fn from(value: ApiErrorResponse) -> Self {
        UserInfoError::Internal(value)
    }
}

pub async fn userinfo(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<UserInfoResponse, UserInfoError> {
    let bearer = extract_bearer(headers).ok_or(UserInfoError::MissingToken)?;

    let (oauth_app, _issuer) = resolve_oauth_app_and_issuer(app_state, headers).await?;
    let pool = app_state
        .db_router
        .reader(common::db_router::ReadConsistency::Strong);

    let hash = sha256_hex(&bearer);
    let token = queries::oauth_runtime::GetRuntimeAccessTokenUserQuery::new(
        oauth_app.deployment_id,
        hash,
    )
    .execute_with_db(pool)
    .await
    .map_err(ApiErrorResponse::from)?
    .ok_or(UserInfoError::InvalidToken("Access token not found"))?;

    // Cross-app token substitution: bearer was minted by a different OAuth
    // app in the same deployment.
    if token.oauth_app_id != oauth_app.id {
        return Err(UserInfoError::InvalidToken(
            "Access token belongs to a different OAuth app",
        ));
    }
    if token.revoked_at.is_some() {
        return Err(UserInfoError::InvalidToken("Access token revoked"));
    }
    if token.expires_at < Utc::now() {
        return Err(UserInfoError::InvalidToken("Access token expired"));
    }
    if token.grant_status != "active" {
        return Err(UserInfoError::InvalidToken("Backing OAuth grant is no longer active"));
    }
    if let Some(expires) = token.grant_expires_at {
        if expires <= Utc::now() {
            return Err(UserInfoError::InvalidToken("Backing OAuth grant has expired"));
        }
    }
    if token.session_deleted_at.is_some() {
        return Err(UserInfoError::InvalidToken("Backing session was terminated"));
    }
    let scopes: Vec<&str> = token.scopes.iter().map(String::as_str).collect();
    if !scopes.contains(&"openid") {
        return Err(UserInfoError::InsufficientScope {
            required: "openid",
            message: "userinfo requires the `openid` scope",
        });
    }
    let user_id = token.user_id.ok_or(UserInfoError::InvalidRequest(
        "Access token has no user subject; service-credential tokens cannot access userinfo",
    ))?;
    let sub = user_id.to_string();

    let user = queries::GetUserDetailsQuery::new(oauth_app.deployment_id, user_id)
        .execute_with_db(pool)
        .await
        .map_err(ApiErrorResponse::from)?;

    let primary_email_verified: Option<bool> = user
        .primary_email_address
        .as_ref()
        .and_then(|primary| {
            user.email_addresses
                .iter()
                .find(|e| &e.email == primary)
                .map(|e| e.verified)
        });

    let mut response = UserInfoResponse {
        sub,
        ..Default::default()
    };
    if scopes.contains(&"profile") {
        response.name = combine_name(&user.first_name, &user.last_name);
        if !user.first_name.is_empty() {
            response.given_name = Some(user.first_name.clone());
        }
        if !user.last_name.is_empty() {
            response.family_name = Some(user.last_name.clone());
        }
        if !user.profile_picture_url.is_empty() {
            response.picture = Some(user.profile_picture_url.clone());
        }
        response.preferred_username = user.username.clone();
        response.updated_at = Some(user.updated_at.timestamp());
    }
    if scopes.contains(&"email") {
        response.email = user.primary_email_address.clone();
        response.email_verified = primary_email_verified;
    }

    Ok(response)
}

#[derive(Debug, Deserialize)]
struct LogoutHintClaims {
    #[allow(dead_code)]
    sub: Option<String>,
    aud: Option<serde_json::Value>,
    iss: Option<String>,
    /// Stringified at serialize-time — JS number precision loss for snowflakes.
    sid: Option<String>,
}

/// id_token_hint verifier per RP-Initiated Logout §3. Skips expiry by design:
/// the spec allows previously-issued tokens, which are by definition often
/// expired by logout time.
fn verify_id_token_hint(
    token: &str,
    keys: &[OAuthAppPublishableKey],
    expected_issuer: &str,
    expected_audience: Option<&str>,
) -> Result<LogoutHintClaims, ApiErrorResponse> {
    let kid = common::utils::jwt::read_kid(token)
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::bad_request("id_token_hint missing kid header"))?;
    let key = keys.iter().find(|k| k.kid == kid).ok_or_else(|| {
        ApiErrorResponse::bad_request(
            "id_token_hint references an unknown or compromised signing key",
        )
    })?;
    let token_data = common::utils::jwt::verify_token_with_claims::<LogoutHintClaims>(
        token,
        &key.algorithm,
        &key.public_key_pem,
        expected_issuer,
        expected_audience,
        /* validate_exp = */ false,
    )
    .map_err(ApiErrorResponse::from)?;
    Ok(token_data.claims)
}

pub async fn logout(
    app_state: &AppState,
    headers: &HeaderMap,
    request: OAuthLogoutRequest,
) -> Result<Redirect, ApiErrorResponse> {
    let (oauth_app, issuer) = resolve_oauth_app_and_issuer(app_state, headers).await?;
    let pool = app_state.db_router.writer();

    let session_id_to_revoke: Option<i64> = if let Some(token) = &request.id_token_hint {
        let reader = app_state
            .db_router
            .reader(common::db_router::ReadConsistency::Strong);
        let candidates = ListOAuthAppPublishableSigningKeysQuery::new(oauth_app.id)
            .execute_with_db(reader)
            .await
            .map_err(ApiErrorResponse::from)?;
        let claims = verify_id_token_hint(
            token,
            &candidates,
            &issuer,
            request.client_id.as_deref(),
        )?;
        let _ = (claims.iss, claims.aud);
        claims.sid.and_then(|s| s.parse::<i64>().ok())
    } else {
        None
    };

    if let Some(sid) = session_id_to_revoke {
        commands::oauth_runtime::RevokeSessionAndCascadeTokens { session_id: sid }
            .execute_with_pool(pool)
            .await
            .map_err(ApiErrorResponse::from)?;
    }

    let redirect_uri = match (&request.post_logout_redirect_uri, &request.client_id) {
        (Some(uri), Some(client_id)) => {
            let client = queries::GetRuntimeOAuthClientByClientIdQuery::new(
                oauth_app.id,
                client_id.clone(),
            )
            .execute_with_db(pool)
            .await
            .map_err(ApiErrorResponse::from)?;
            let Some(client) = client else {
                return Err(ApiErrorResponse::bad_request("Unknown client_id"));
            };
            if !client_allows_post_logout_redirect(&client, uri) {
                return Err(ApiErrorResponse::bad_request(
                    "post_logout_redirect_uri not registered for this client",
                ));
            }
            Some(uri.clone())
        }
        _ => None,
    };

    let target = if let Some(uri) = redirect_uri {
        if let Some(state) = &request.state {
            if uri.contains('?') {
                format!("{}&state={}", uri, urlencoding::encode(state))
            } else {
                format!("{}?state={}", uri, urlencoding::encode(state))
            }
        } else {
            uri
        }
    } else {
        format!("{}/sign-in?signed_out=1", issuer)
    };

    Ok(Redirect::to(&target))
}

fn client_allows_post_logout_redirect(
    client: &queries::RuntimeOAuthClientData,
    uri: &str,
) -> bool {
    client
        .post_logout_redirect_uris
        .iter()
        .any(|registered| registered == uri)
}

pub struct IdTokenBuildContext {
    pub issuer: String,
    pub client_id: String,
    pub deployment_id: i64,
    pub user_id: i64,
    pub session_id: Option<i64>,
    pub auth_time: i64,
    pub nonce: Option<String>,
    pub access_token: String,
    pub scopes: Vec<String>,
}

pub async fn build_id_token(
    app_state: &AppState,
    oauth_app_id: i64,
    ctx: IdTokenBuildContext,
) -> Result<String, ApiErrorResponse> {
    let key = ensure_active_signing_key(app_state, oauth_app_id).await?;

    let scopes_set: std::collections::HashSet<&str> =
        ctx.scopes.iter().map(String::as_str).collect();
    let want_profile = scopes_set.contains("profile");
    let want_email = scopes_set.contains("email");

    // Skip the user-details fetch entirely when neither claim group is asked
    // for — openid-only tokens carry just `sub` and the protocol claims.
    let user_details = if want_profile || want_email {
        let reader = app_state
            .db_router
            .reader(common::db_router::ReadConsistency::Strong);
        Some(
            queries::GetUserDetailsQuery::new(ctx.deployment_id, ctx.user_id)
                .execute_with_db(reader)
                .await
                .map_err(ApiErrorResponse::from)?,
        )
    } else {
        None
    };

    let now = Utc::now().timestamp();
    let exp = now + 15 * 60;

    let claims = IdTokenClaims {
        iss: ctx.issuer,
        sub: ctx.user_id.to_string(),
        aud: ctx.client_id,
        exp,
        iat: now,
        auth_time: ctx.auth_time,
        nonce: ctx.nonce,
        at_hash: Some(compute_at_hash(&ctx.access_token)),
        sid: ctx.session_id.map(|id| id.to_string()),
        name: user_details
            .as_ref()
            .filter(|_| want_profile)
            .and_then(|u| combine_name(&u.first_name, &u.last_name)),
        given_name: user_details
            .as_ref()
            .filter(|_| want_profile)
            .map(|u| u.first_name.clone())
            .filter(|s| !s.is_empty()),
        family_name: user_details
            .as_ref()
            .filter(|_| want_profile)
            .map(|u| u.last_name.clone())
            .filter(|s| !s.is_empty()),
        picture: user_details
            .as_ref()
            .filter(|_| want_profile)
            .map(|u| u.profile_picture_url.clone())
            .filter(|s| !s.is_empty()),
        preferred_username: user_details
            .as_ref()
            .filter(|_| want_profile)
            .and_then(|u| u.username.clone()),
        email: user_details
            .as_ref()
            .filter(|_| want_email)
            .and_then(|u| u.primary_email_address.clone()),
        email_verified: user_details
            .as_ref()
            .filter(|_| want_email)
            .and_then(|u| {
                let primary = u.primary_email_address.as_ref()?;
                u.email_addresses
                    .iter()
                    .find(|e| &e.email == primary)
                    .map(|e| e.verified)
            }),
    };

    common::utils::jwt::sign_token_with_kid(&claims, &key.algorithm, &key.private_key_pem, &key.kid)
        .map_err(ApiErrorResponse::from)
}

fn combine_name(first: &str, last: &str) -> Option<String> {
    let trimmed = format!("{} {}", first, last).trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// OIDC Core §3.1.3.6 `at_hash`: base64url(no-pad) of left 128 bits of
/// SHA-256(access_token).
fn compute_at_hash(access_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(access_token.as_bytes());
    let digest = hasher.finalize();
    let half = &digest[..digest.len() / 2];
    URL_SAFE_NO_PAD.encode(half)
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(axum::http::header::AUTHORIZATION)?;
    let s = v.to_str().ok()?;
    let stripped = s.strip_prefix("Bearer ").or_else(|| s.strip_prefix("bearer "))?;
    if stripped.trim().is_empty() {
        None
    } else {
        Some(stripped.trim().to_string())
    }
}

fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}
