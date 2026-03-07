use axum::http::{HeaderMap, header::AUTHORIZATION};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use commands::CreateOAuthClientGrantCommand;
use common::{
    db_router::ReadConsistency, error::AppError, state::AppState, utils::jwt::verify_token,
};
use core::cmp::Ordering;
use dto::json::oauth_runtime::OAuthTokenRequest;
use hmac::{Hmac, Mac};
use models::api_key::OAuthScopeDefinition;
use queries::{
    GetRuntimeApiAuthUserIdByAppSlugQuery, GetRuntimeOAuthClientByClientIdQuery,
    ResolveOAuthAppByFqdnQuery, ResolveRuntimeOAuthGrantQuery,
    ValidateRuntimeResourceEntitlementQuery,
};
use rand::RngCore;
use sha2::{Digest, Sha256};

use super::types::{ClientAssertionClaims, GrantValidationResult, OAuthConsentRequestTokenClaims};

pub(crate) fn oauth_consent_handoff_redis_key(handoff_id: &str) -> String {
    format!("oauth:consent:handoff:{handoff_id}")
}

pub(crate) fn oauth_consent_backend_base_url(backend_host: &str) -> String {
    let host = backend_host.trim();
    format!("https://{host}")
}

pub(crate) fn derive_shared_secret(purpose: &str) -> Result<String, AppError> {
    let encryption_key = std::env::var("ENCRYPTION_KEY").map_err(|_| {
        AppError::Internal("ENCRYPTION_KEY is required for oauth consent flow".to_string())
    })?;
    let mut hasher = Sha256::new();
    hasher.update(purpose.as_bytes());
    hasher.update(b":");
    hasher.update(encryption_key.trim().as_bytes());
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize()))
}

pub(crate) async fn resolve_oauth_app_from_host(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<queries::RuntimeOAuthAppData, AppError> {
    let host = resolve_host(headers)
        .and_then(normalize_fqdn_host)
        .ok_or_else(|| AppError::NotFound("OAuth app not found for host".to_string()))?;

    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    ResolveOAuthAppByFqdnQuery::new(host.to_string())
        .execute_with_db(reader)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth app not found for host".to_string()))
}

pub(crate) async fn authenticate_client(
    app_state: &AppState,
    headers: &HeaderMap,
    issuer: &str,
    request: &OAuthTokenRequest,
    oauth_app_id: i64,
    endpoint_path: &str,
) -> Result<queries::RuntimeOAuthClientData, AppError> {
    let (basic_client_id, basic_secret) = extract_basic_credentials(headers)?;
    let client_id = basic_client_id
        .or_else(|| request.client_id.clone())
        .ok_or(AppError::Unauthorized)?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app_id, client_id)
        .execute_with_db(reader)
        .await?
        .ok_or(AppError::Unauthorized)?;
    if !client.is_active {
        return Err(AppError::Unauthorized);
    }
    let expected_assertion_audience = format!("{}{}", issuer, endpoint_path);

    match client.client_auth_method.as_str() {
        "none" => Ok(client),
        "client_secret_basic" => {
            let secret = basic_secret.ok_or(AppError::Unauthorized)?;
            validate_secret_hash(client.client_secret_hash.as_deref(), &secret)?;
            Ok(client)
        }
        "client_secret_post" => {
            let secret = request
                .client_secret
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or(AppError::Unauthorized)?;
            validate_secret_hash(client.client_secret_hash.as_deref(), secret)?;
            Ok(client)
        }
        "client_secret_jwt" => {
            let secret_encrypted = client
                .client_secret_encrypted
                .as_deref()
                .ok_or(AppError::Unauthorized)?;
            validate_assertion_type(request.client_assertion_type.as_deref())?;
            let assertion = request
                .client_assertion
                .as_deref()
                .ok_or(AppError::Unauthorized)?;
            let alg = client
                .token_endpoint_auth_signing_alg
                .as_deref()
                .unwrap_or("HS256");
            if !matches!(alg, "HS256" | "HS384" | "HS512") {
                return Err(AppError::Unauthorized);
            }
            let secret = app_state
                .encryption_service
                .decrypt(secret_encrypted)
                .map_err(|_| AppError::Unauthorized)?;
            let claims = verify_token::<ClientAssertionClaims>(assertion, alg, &secret)?.claims;
            validate_assertion_claims(&claims, &client.client_id, &expected_assertion_audience)?;
            enforce_assertion_replay_protection(app_state, &client.client_id, &claims).await?;
            Ok(client)
        }
        "private_key_jwt" => {
            validate_assertion_type(request.client_assertion_type.as_deref())?;
            let assertion = request
                .client_assertion
                .as_deref()
                .ok_or(AppError::Unauthorized)?;
            let public_key = client
                .jwks
                .as_ref()
                .and_then(|j| j.public_key_pem())
                .ok_or_else(|| {
                    AppError::BadRequest(
                        "private_key_jwt requires a registered PEM key in jwks".to_string(),
                    )
                })?;
            let alg = client
                .token_endpoint_auth_signing_alg
                .as_deref()
                .unwrap_or("RS256");
            if !matches!(
                alg,
                "RS256" | "RS384" | "RS512" | "ES256" | "ES384" | "ES512"
            ) {
                return Err(AppError::Unauthorized);
            }
            let claims = verify_token::<ClientAssertionClaims>(assertion, alg, &public_key)?.claims;
            validate_assertion_claims(&claims, &client.client_id, &expected_assertion_audience)?;
            enforce_assertion_replay_protection(app_state, &client.client_id, &claims).await?;
            Ok(client)
        }
        _ => Err(AppError::Unauthorized),
    }
}

pub(crate) async fn validate_grant_and_entitlement(
    app_state: &AppState,
    deployment_id: i64,
    oauth_client_id: i64,
    oauth_grant_id: Option<i64>,
    app_slug: String,
    scopes: Vec<String>,
    resource: Option<String>,
    scope_definitions: &[OAuthScopeDefinition],
) -> Result<GrantValidationResult, AppError> {
    let grant = if let Some(grant_id) = oauth_grant_id {
        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        ResolveRuntimeOAuthGrantQuery::by_grant_id(deployment_id, oauth_client_id, grant_id)
            .execute_with_db(reader)
            .await?
    } else {
        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        ResolveRuntimeOAuthGrantQuery::by_scope_match(
            deployment_id,
            oauth_client_id,
            app_slug.clone(),
            scopes.clone(),
            resource.clone(),
        )
        .execute_with_db(reader)
        .await?
    };

    if grant.revoked {
        return Ok(GrantValidationResult::Revoked);
    }
    if !grant.active {
        return Ok(GrantValidationResult::MissingOrInsufficient);
    }

    let Some(resource) = resource else {
        return Ok(GrantValidationResult::Active);
    };

    let required_permissions =
        required_permissions_for_resource(scope_definitions, &scopes, &resource);

    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let user_id = GetRuntimeApiAuthUserIdByAppSlugQuery::new(deployment_id, app_slug)
        .execute_with_db(reader)
        .await?;
    let Some(user_id) = user_id else {
        return Ok(GrantValidationResult::MissingOrInsufficient);
    };

    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let entitled = ValidateRuntimeResourceEntitlementQuery::new(
        deployment_id,
        user_id,
        resource,
        required_permissions,
    )
    .execute_with_db(reader)
    .await?;
    if entitled {
        Ok(GrantValidationResult::Active)
    } else {
        Ok(GrantValidationResult::MissingOrInsufficient)
    }
}

pub(crate) async fn ensure_or_create_grant_coverage(
    app_state: &AppState,
    deployment_id: i64,
    oauth_client_id: i64,
    app_slug: String,
    scopes: Vec<String>,
    resource: String,
    user_id: i64,
) -> Result<i64, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let resolved = ResolveRuntimeOAuthGrantQuery::by_scope_match(
        deployment_id,
        oauth_client_id,
        app_slug.clone(),
        scopes.clone(),
        Some(resource.clone()),
    )
    .execute_with_db(reader)
    .await?;
    if let Some(grant_id) = resolved.active_grant_id {
        return Ok(grant_id);
    }
    if resolved.revoked {
        return Err(AppError::Forbidden(
            "Grant is revoked for requested scopes/resource".to_string(),
        ));
    }

    let writer = app_state.db_router.writer();
    let created = CreateOAuthClientGrantCommand {
        grant_id: Some(
            app_state
                .sf
                .next_id()
                .map_err(|e| AppError::Internal(e.to_string()))? as i64,
        ),
        deployment_id,
        api_auth_app_slug: app_slug,
        oauth_client_id,
        resource,
        scopes,
        granted_by_user_id: Some(user_id),
        expires_at: None,
    }
    .execute_with_db(writer)
    .await?;
    Ok(created.id)
}

pub(super) fn validate_secret_hash(
    stored_hash: Option<&str>,
    provided_secret: &str,
) -> Result<(), AppError> {
    let Some(stored_hash) = stored_hash else {
        return Err(AppError::Unauthorized);
    };
    let provided_hash = hash_value(provided_secret);
    match stored_hash.len().cmp(&provided_hash.len()) {
        Ordering::Equal => {
            let mut diff: u8 = 0;
            for (a, b) in stored_hash
                .as_bytes()
                .iter()
                .zip(provided_hash.as_bytes().iter())
            {
                diff |= a ^ b;
            }
            if diff == 0 {
                Ok(())
            } else {
                Err(AppError::Unauthorized)
            }
        }
        _ => Err(AppError::Unauthorized),
    }
}

pub(super) fn validate_assertion_type(assertion_type: Option<&str>) -> Result<(), AppError> {
    if assertion_type.unwrap_or_default()
        != "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
    {
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

pub(super) fn validate_assertion_claims(
    claims: &ClientAssertionClaims,
    expected_client_id: &str,
    expected_audience: &str,
) -> Result<(), AppError> {
    const ASSERTION_MAX_LIFETIME_SECONDS: i64 = 300;
    const CLOCK_SKEW_SECONDS: i64 = 60;
    let now = Utc::now().timestamp();

    if claims.sub.as_deref() != Some(expected_client_id) {
        return Err(AppError::Unauthorized);
    }
    if claims.iss.as_deref() != Some(expected_client_id) {
        return Err(AppError::Unauthorized);
    }
    let aud_ok = claims
        .aud
        .as_ref()
        .is_some_and(|aud| audience_matches(aud, expected_audience));
    if !aud_ok {
        return Err(AppError::Unauthorized);
    }
    if claims.exp.is_none() || claims.iat.is_none() || claims.jti.is_none() {
        return Err(AppError::Unauthorized);
    }
    let exp = claims.exp.ok_or(AppError::Unauthorized)?;
    let iat = claims.iat.ok_or(AppError::Unauthorized)?;
    if exp <= now || exp > now + ASSERTION_MAX_LIFETIME_SECONDS {
        return Err(AppError::Unauthorized);
    }
    if iat > now + CLOCK_SKEW_SECONDS || iat < now - ASSERTION_MAX_LIFETIME_SECONDS {
        return Err(AppError::Unauthorized);
    }
    if exp - iat > ASSERTION_MAX_LIFETIME_SECONDS {
        return Err(AppError::Unauthorized);
    }
    if let Some(nbf) = claims.nbf {
        if nbf > now + CLOCK_SKEW_SECONDS {
            return Err(AppError::Unauthorized);
        }
    }
    Ok(())
}

pub(super) fn audience_matches(aud: &serde_json::Value, expected: &str) -> bool {
    match aud {
        serde_json::Value::String(s) => s == expected,
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|s| s == expected)),
        _ => false,
    }
}

pub(super) async fn enforce_assertion_replay_protection(
    app_state: &AppState,
    client_id: &str,
    claims: &ClientAssertionClaims,
) -> Result<(), AppError> {
    const ASSERTION_MAX_LIFETIME_SECONDS: i64 = 300;
    let jti = claims
        .jti
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or(AppError::Unauthorized)?;
    let exp = claims.exp.ok_or(AppError::Unauthorized)?;
    let now = Utc::now().timestamp();
    if exp <= now {
        return Err(AppError::Unauthorized);
    }
    let ttl = (exp - now).clamp(1, ASSERTION_MAX_LIFETIME_SECONDS);
    let redis_key = format!("oauth:client-assertion:jti:{}:{}", client_id, jti);

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|_| AppError::Internal("Failed to connect redis".to_string()))?;
    let inserted: Option<String> = redis::cmd("SET")
        .arg(&redis_key)
        .arg("1")
        .arg("NX")
        .arg("EX")
        .arg(ttl)
        .query_async(&mut redis_conn)
        .await
        .map_err(|_| AppError::Internal("Failed to store assertion jti".to_string()))?;
    if inserted.is_none() {
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

pub(super) fn extract_basic_credentials(
    headers: &HeaderMap,
) -> Result<(Option<String>, Option<String>), AppError> {
    let auth = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .map(str::trim);
    let Some(auth) = auth else {
        return Ok((None, None));
    };
    if !auth.starts_with("Basic ") {
        return Ok((None, None));
    }
    let raw = auth.trim_start_matches("Basic ").trim();
    let decoded = STANDARD.decode(raw).map_err(|_| AppError::Unauthorized)?;
    let pair = String::from_utf8(decoded).map_err(|_| AppError::Unauthorized)?;
    let mut parts = pair.splitn(2, ':');
    let client_id = parts.next().unwrap_or_default().to_string();
    let secret = parts.next().unwrap_or_default().to_string();
    if client_id.is_empty() || secret.is_empty() {
        return Err(AppError::Unauthorized);
    }
    Ok((Some(client_id), Some(secret)))
}

pub(crate) fn verify_pkce(
    code_challenge: Option<&str>,
    code_challenge_method: Option<&str>,
    code_verifier: Option<&str>,
) -> Result<(), AppError> {
    let Some(challenge) = code_challenge else {
        return Ok(());
    };
    let verifier = code_verifier
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::BadRequest("code_verifier is required".to_string()))?;
    let method = code_challenge_method.unwrap_or("S256");
    if !method.eq_ignore_ascii_case("S256") {
        return Err(AppError::BadRequest(
            "unsupported code_challenge_method".to_string(),
        ));
    }
    let digest = Sha256::digest(verifier.as_bytes());
    let transformed = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    let valid = transformed == challenge;
    if !valid {
        return Err(AppError::BadRequest("invalid code_verifier".to_string()));
    }
    Ok(())
}

pub(crate) fn is_valid_resource_indicator(resource: &str) -> bool {
    url::Url::parse(resource).is_ok_and(|uri| !uri.scheme().is_empty())
}

pub(crate) fn is_valid_granted_resource_indicator(resource: &str) -> bool {
    if resource.starts_with("urn:wacht:organization:") {
        return resource
            .trim_start_matches("urn:wacht:organization:")
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .is_some();
    }
    if resource.starts_with("urn:wacht:workspace:") {
        return resource
            .trim_start_matches("urn:wacht:workspace:")
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .is_some();
    }
    if resource.starts_with("urn:wacht:user:") {
        return resource
            .trim_start_matches("urn:wacht:user:")
            .parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .is_some();
    }
    false
}

pub(super) fn required_permissions_for_resource(
    scope_definitions: &[OAuthScopeDefinition],
    scopes: &[String],
    resource: &str,
) -> Vec<String> {
    let resource_type = if resource.starts_with("urn:wacht:organization:") {
        "organization"
    } else if resource.starts_with("urn:wacht:workspace:") {
        "workspace"
    } else {
        ""
    };
    if resource_type.is_empty() {
        return Vec::new();
    }

    let mut permissions = Vec::new();
    for scope in scopes {
        let Some(def) = scope_definitions.iter().find(|d| d.scope == *scope) else {
            continue;
        };
        let category = def.category.trim().to_ascii_lowercase();
        if category != resource_type {
            continue;
        }
        let permission = match resource_type {
            "organization" => def.organization_permission.as_deref(),
            "workspace" => def.workspace_permission.as_deref(),
            _ => None,
        };
        if let Some(value) = permission.map(str::trim).filter(|v| !v.is_empty()) {
            permissions.push(value.to_string());
        }
    }
    permissions.sort_unstable();
    permissions.dedup();
    permissions
}

pub(crate) fn parse_scope_string(scope: Option<&str>) -> Vec<String> {
    scope
        .unwrap_or_default()
        .split(' ')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub(crate) fn hash_value(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn resolve_issuer_from_oauth_app(
    oauth_app: &queries::RuntimeOAuthAppData,
) -> Result<String, AppError> {
    let fqdn = oauth_app.fqdn.trim();
    if fqdn.is_empty() {
        return Err(AppError::BadRequest(
            "oauth app fqdn is required".to_string(),
        ));
    }
    if fqdn.contains("://") || fqdn.contains(':') || fqdn.contains('/') {
        return Err(AppError::BadRequest(
            "oauth app fqdn must be a bare host without scheme, port, or path".to_string(),
        ));
    }
    Ok(format!("https://{}", fqdn))
}

pub(super) fn resolve_host(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("host").and_then(|v| v.to_str().ok()))
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

pub(super) fn normalize_fqdn_host(host: &str) -> Option<&str> {
    let value = host.trim();
    if value.is_empty() {
        return None;
    }

    let stripped = if value.starts_with('[') {
        value
            .strip_prefix('[')
            .and_then(|v| v.split(']').next())
            .unwrap_or(value)
    } else {
        value.split(':').next().unwrap_or(value)
    }
    .trim();

    if stripped.is_empty() {
        None
    } else {
        Some(stripped)
    }
}

pub(crate) fn ensure_registration_access_token(
    headers: &HeaderMap,
    expected_hash: Option<&str>,
) -> Result<(), AppError> {
    let Some(expected_hash) = expected_hash else {
        return Err(AppError::Unauthorized);
    };
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or(AppError::Unauthorized)?;
    let provided_hash = hash_value(token);
    match expected_hash.len().cmp(&provided_hash.len()) {
        Ordering::Equal => {
            let mut diff: u8 = 0;
            for (a, b) in expected_hash
                .as_bytes()
                .iter()
                .zip(provided_hash.as_bytes().iter())
            {
                diff |= a ^ b;
            }
            if diff != 0 {
                return Err(AppError::Unauthorized);
            }
        }
        _ => return Err(AppError::Unauthorized),
    }
    Ok(())
}

pub(crate) fn generate_registration_access_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!(
        "orat_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

pub(crate) fn generate_prefixed_token(prefix: &str, bytes_len: usize) -> String {
    let mut bytes = vec![0u8; bytes_len];
    rand::rng().fill_bytes(&mut bytes);
    format!(
        "{}_{}",
        prefix,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

pub(crate) fn client_secret_expires_at_for_method(client_auth_method: &str) -> Option<i64> {
    match client_auth_method {
        "none" | "private_key_jwt" => None,
        _ => Some(0),
    }
}

pub(super) fn oauth_consent_request_secret() -> Result<String, AppError> {
    derive_shared_secret("oauth-consent-request-v1")
}

pub(crate) fn sign_oauth_consent_request_token(
    claims: &OAuthConsentRequestTokenClaims,
) -> Result<String, AppError> {
    let payload_json =
        serde_json::to_vec(claims).map_err(|e| AppError::Serialization(e.to_string()))?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json);
    let signature = sign_payload(payload.as_bytes())?;
    Ok(format!("ocrt.{}.{}", payload, signature))
}

pub(crate) fn verify_oauth_consent_request_token(
    token: &str,
) -> Result<OAuthConsentRequestTokenClaims, AppError> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 || parts[0] != "ocrt" {
        return Err(AppError::Unauthorized);
    }

    let payload = parts[1];
    let provided_sig = parts[2];
    let provided_sig_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(provided_sig)
        .map_err(|_| AppError::Unauthorized)?;
    type HmacSha256 = Hmac<Sha256>;
    let secret = oauth_consent_request_secret()?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| AppError::Internal(e.to_string()))?;
    mac.update(payload.as_bytes());
    if mac.verify_slice(&provided_sig_bytes).is_err() {
        return Err(AppError::Unauthorized);
    }

    let payload_bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| AppError::Unauthorized)?;
    let claims: OAuthConsentRequestTokenClaims =
        serde_json::from_slice(&payload_bytes).map_err(|_| AppError::Unauthorized)?;

    if claims.exp <= Utc::now().timestamp() {
        return Err(AppError::Unauthorized);
    }

    Ok(claims)
}

pub(super) fn sign_payload(payload: &[u8]) -> Result<String, AppError> {
    type HmacSha256 = Hmac<Sha256>;
    let secret = oauth_consent_request_secret()?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| AppError::Internal(e.to_string()))?;
    mac.update(payload);
    let signature = mac.finalize().into_bytes();
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature))
}

pub(crate) fn append_oauth_redirect_params(
    redirect_uri: String,
    params: &[(&str, String)],
    state: Option<String>,
    issuer: Option<String>,
) -> String {
    let mut uri = match url::Url::parse(&redirect_uri) {
        Ok(u) => u,
        Err(_) => return redirect_uri,
    };
    {
        let mut query = uri.query_pairs_mut();
        for (k, v) in params {
            query.append_pair(k, v);
        }
        if let Some(state) = state {
            query.append_pair("state", &state);
        }
        if let Some(issuer) = issuer {
            query.append_pair("iss", &issuer);
        }
    }
    uri.to_string()
}
