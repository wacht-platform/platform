use axum::{
    Json,
    extract::{Form, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::AUTHORIZATION, header::WWW_AUTHENTICATE},
    response::IntoResponse,
    response::Redirect,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use commands::api_key_app::EnsureUserApiAuthAppCommand;
use commands::{
    Command, ConsumeOAuthAuthorizationCode, CreateOAuthClientCommand,
    CreateOAuthClientGrantCommand, DeactivateOAuthClient, EnqueueOAuthGrantLastUsed,
    IssueOAuthAuthorizationCode, IssueOAuthTokenPair, RevokeOAuthAccessTokenByHash,
    RevokeOAuthRefreshTokenByHash, RevokeOAuthRefreshTokenById, RevokeOAuthRefreshTokenFamily,
    RevokeOAuthTokensByGrant, SetOAuthClientRegistrationAccessToken,
    SetOAuthRefreshTokenReplacement, UpdateOAuthClientSettings,
};
use common::{error::AppError, state::AppState, utils::jwt::verify_token};
use core::cmp::Ordering;
use dto::json::oauth_runtime::{
    OAuthAuthorizeInitiatedResponse, OAuthAuthorizeRequest, OAuthConsentSubmitRequest,
    OAuthDynamicClientRegistrationRequest, OAuthDynamicClientRegistrationResponse,
    OAuthDynamicClientUpdateRequest, OAuthErrorResponse, OAuthIntrospectRequest,
    OAuthIntrospectResponse, OAuthProtectedResourceMetadataResponse, OAuthRegisterPathParams,
    OAuthRevokeRequest, OAuthRevokeResponse, OAuthServerMetadataResponse, OAuthTokenRequest,
    OAuthTokenResponse,
};
use hmac::{Hmac, Mac};
use models::api_key::OAuthScopeDefinition;
use queries::Query as QueryTrait;
use queries::{
    GetRuntimeApiAuthUserIdByAppSlugQuery, GetRuntimeAuthorizationCodeForExchangeQuery,
    GetRuntimeDeploymentHostsByIdQuery, GetRuntimeIntrospectionDataQuery,
    GetRuntimeOAuthClientByClientIdQuery, GetRuntimeRefreshTokenForExchangeQuery,
    ResolveOAuthAppByFqdnQuery, ResolveRuntimeOAuthGrantQuery,
    ValidateRuntimeResourceEntitlementQuery,
};
use rand::RngCore;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::application::response::{ApiErrorResponse, ApiResult};

#[derive(Debug, Deserialize)]
struct ClientAssertionClaims {
    iss: Option<String>,
    sub: Option<String>,
    aud: Option<serde_json::Value>,
    exp: Option<i64>,
    iat: Option<i64>,
    nbf: Option<i64>,
    jti: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OAuthConsentRequestTokenClaims {
    exp: i64,
    iat: i64,
    jti: String,
    deployment_id: i64,
    oauth_client_id: i64,
    client_id: String,
    redirect_uri: String,
    scopes: Vec<String>,
    resource: Option<String>,
    state: Option<String>,
    code_challenge: Option<String>,
    code_challenge_method: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OAuthConsentHandoffPayload {
    request_token: String,
    issuer: String,
    deployment_id: i64,
    oauth_client_id: i64,
    client_id: String,
    redirect_uri: String,
    scopes: Vec<String>,
    scope_definitions: Vec<OAuthScopeDefinition>,
    resource: Option<String>,
    resource_options: Vec<String>,
    state: Option<String>,
    expires_at: i64,
    client_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GrantValidationResult {
    Active,
    Revoked,
    MissingOrInsufficient,
}

pub async fn oauth_server_metadata(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<OAuthServerMetadataResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let active_scopes = oauth_app.active_scopes();
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;

    Ok(OAuthServerMetadataResponse {
        issuer: issuer.clone(),
        authorization_endpoint: format!("{}/oauth/authorize", issuer),
        token_endpoint: format!("{}/oauth/token", issuer),
        revocation_endpoint: format!("{}/oauth/revoke", issuer),
        introspection_endpoint: format!("{}/oauth/introspect", issuer),
        registration_endpoint: format!("{}/oauth/register", issuer),
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        token_endpoint_auth_methods_supported: vec![
            "client_secret_basic".to_string(),
            "client_secret_post".to_string(),
            "client_secret_jwt".to_string(),
            "private_key_jwt".to_string(),
            "none".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string()],
        scopes_supported: active_scopes,
    }
    .into())
}

pub async fn oauth_protected_resource_metadata(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<OAuthProtectedResourceMetadataResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;

    Ok(OAuthProtectedResourceMetadataResponse {
        resource: issuer.clone(),
        authorization_servers: vec![issuer],
        bearer_methods_supported: vec!["header".to_string()],
        scopes_supported: oauth_app.active_scopes(),
    }
    .into())
}

pub async fn oauth_authorize_get(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(request): Query<OAuthAuthorizeRequest>,
) -> Result<Redirect, ApiErrorResponse> {
    let request_ctx = request.clone();
    match authorize_impl(&app_state, &headers, request).await {
        Ok(initiated) => Ok(Redirect::temporary(&initiated.consent_url)),
        Err(err) => {
            if let Some(redirect) =
                try_build_authorize_error_redirect(&app_state, &headers, &request_ctx, &err).await
            {
                return Ok(Redirect::temporary(&redirect));
            }
            Err(err)
        }
    }
}

async fn authorize_impl(
    app_state: &AppState,
    headers: &HeaderMap,
    request: OAuthAuthorizeRequest,
) -> Result<OAuthAuthorizeInitiatedResponse, ApiErrorResponse> {
    let response_type = request
        .response_type
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing response_type"))?;
    if response_type != "code" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Only response_type=code is supported",
        )
            .into());
    }
    let client_id = request
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing client_id"))?
        .to_string();
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, "missing redirect_uri"))?
        .to_string();

    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await?;
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, client_id)
        .execute(app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;
    if !client.is_active {
        return Err((StatusCode::BAD_REQUEST, "OAuth client is inactive").into());
    }
    if !client.grant_types.iter().any(|g| g == "authorization_code") {
        return Err((
            StatusCode::BAD_REQUEST,
            "authorization_code is not allowed for this client",
        )
            .into());
    }
    if !client.redirect_uris.iter().any(|u| u == &redirect_uri) {
        return Err((
            StatusCode::BAD_REQUEST,
            "redirect_uri is not registered for this client",
        )
            .into());
    }
    if client.client_auth_method == "none" {
        let code_challenge = request
            .code_challenge
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    "code_challenge is required for public clients",
                )
            })?;
        if code_challenge.len() < 43 || code_challenge.len() > 128 {
            return Err((StatusCode::BAD_REQUEST, "invalid code_challenge").into());
        }
        let method = request
            .code_challenge_method
            .as_deref()
            .unwrap_or("S256")
            .trim();
        if method != "S256" {
            return Err((StatusCode::BAD_REQUEST, "unsupported code_challenge_method").into());
        }
    }

    let final_scopes = parse_scope_string(request.scope.as_deref());
    let active_scopes = oauth_app.active_scopes();
    let unsupported_scopes: Vec<String> = final_scopes
        .iter()
        .filter(|scope| !active_scopes.iter().any(|s| s == *scope))
        .cloned()
        .collect();
    if !unsupported_scopes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Unsupported scopes requested: {}",
                unsupported_scopes.join(", ")
            ),
        )
            .into());
    }
    let final_resource = request
        .resource
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());
    if let Some(resource) = final_resource.as_deref() {
        if !is_valid_resource_indicator(resource) {
            return Err((
                StatusCode::BAD_REQUEST,
                "resource must be an absolute URI (e.g. urn:wacht:workspace:123)",
            )
                .into());
        }
    }
    let resource_options = final_resource
        .as_ref()
        .map(|r| vec![r.clone()])
        .unwrap_or_default();
    let scope_definitions: Vec<OAuthScopeDefinition> = final_scopes
        .iter()
        .map(|scope| {
            oauth_app
                .scope_definitions
                .iter()
                .find(|definition| definition.scope == *scope)
                .cloned()
                .unwrap_or_else(|| OAuthScopeDefinition {
                    scope: scope.clone(),
                    display_name: scope.clone(),
                    description: String::new(),
                    archived: false,
                    category: String::new(),
                    organization_permission: None,
                    workspace_permission: None,
                })
        })
        .collect();

    let iat = Utc::now().timestamp();
    let exp = iat + 600;
    let claims = OAuthConsentRequestTokenClaims {
        exp,
        iat,
        jti: generate_prefixed_token("ocrq", 16),
        deployment_id: oauth_app.deployment_id,
        oauth_client_id: client.id,
        client_id: client.client_id.clone(),
        redirect_uri,
        scopes: final_scopes,
        resource: final_resource,
        state: request.state,
        code_challenge: request.code_challenge,
        code_challenge_method: request.code_challenge_method,
    };
    let request_token = sign_oauth_consent_request_token(&claims)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let deployment_hosts = GetRuntimeDeploymentHostsByIdQuery::new(oauth_app.deployment_id)
        .execute(app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Deployment not found for OAuth app"))?;

    let handoff_id = generate_prefixed_token("och", 18);
    let handoff_key = oauth_consent_handoff_redis_key(&handoff_id);
    let handoff_payload = OAuthConsentHandoffPayload {
        request_token,
        issuer: issuer.clone(),
        deployment_id: oauth_app.deployment_id,
        oauth_client_id: client.id,
        client_id: claims.client_id,
        redirect_uri: claims.redirect_uri,
        scopes: claims.scopes,
        scope_definitions,
        resource: claims.resource,
        resource_options,
        state: claims.state,
        expires_at: claims.exp,
        client_name: client.client_name,
    };

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to connect redis: {e}"),
            )
        })?;
    redis_conn
        .set_ex::<_, _, ()>(
            &handoff_key,
            serde_json::to_string(&handoff_payload).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to encode handoff",
                )
            })?,
            600,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to store oauth handoff: {e}"),
            )
        })?;

    let backend_base = oauth_consent_backend_base_url(&deployment_hosts.backend_host);
    let consent_url = format!(
        "{}/oauth/consent/init?handoff_id={}",
        backend_base, handoff_id
    );
    Ok(OAuthAuthorizeInitiatedResponse {
        consent_url,
        expires_in: 600,
    })
}

async fn try_build_authorize_error_redirect(
    app_state: &AppState,
    headers: &HeaderMap,
    request: &OAuthAuthorizeRequest,
    err: &ApiErrorResponse,
) -> Option<String> {
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    if redirect_uri.is_empty() {
        return None;
    }

    let client_id = request
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?
        .to_string();
    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await.ok()?;
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, client_id)
        .execute(app_state)
        .await
        .ok()??;
    let redirect_registered = client.redirect_uris.iter().any(|u| u == redirect_uri);
    if !redirect_registered {
        return None;
    }

    let description = err
        .errors
        .first()
        .map(|e| e.message.clone())
        .unwrap_or_else(|| "invalid_request".to_string());
    let error_code = match description.as_str() {
        message if message.starts_with("Unsupported scopes requested") => "invalid_scope",
        "authorization_code is not allowed for this client" | "OAuth client is inactive" => {
            "unauthorized_client"
        }
        _ if err.staus_code == StatusCode::INTERNAL_SERVER_ERROR => "server_error",
        _ => "invalid_request",
    };

    let issuer = resolve_issuer_from_oauth_app(&oauth_app).ok();
    Some(append_oauth_redirect_params(
        redirect_uri.to_string(),
        &[
            ("error", error_code.to_string()),
            ("error_description", description),
        ],
        request.state.clone(),
        issuer,
    ))
}

fn oauth_consent_handoff_redis_key(handoff_id: &str) -> String {
    format!("oauth:consent:handoff:{handoff_id}")
}

fn oauth_consent_backend_base_url(backend_host: &str) -> String {
    let host = backend_host.trim();
    format!("https://{host}")
}

fn derive_shared_secret(purpose: &str) -> Result<String, AppError> {
    let encryption_key = std::env::var("ENCRYPTION_KEY").map_err(|_| {
        AppError::Internal("ENCRYPTION_KEY is required for oauth consent flow".to_string())
    })?;
    let mut hasher = Sha256::new();
    hasher.update(purpose.as_bytes());
    hasher.update(b":");
    hasher.update(encryption_key.trim().as_bytes());
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hasher.finalize()))
}

fn validate_consent_submit_secret(headers: &HeaderMap) -> Result<(), ApiErrorResponse> {
    let expected = derive_shared_secret("oauth-consent-submit-v1").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "OAuth consent submit secret is not configured",
        )
    })?;
    let provided = headers
        .get("X-OAuth-Consent-Secret")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Missing OAuth consent submit secret",
            )
        })?;
    if provided != expected {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Invalid OAuth consent submit secret",
        )
            .into());
    }
    Ok(())
}

pub async fn oauth_consent_submit(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthConsentSubmitRequest>,
) -> Result<Redirect, ApiErrorResponse> {
    validate_consent_submit_secret(&headers)?;
    let claims = verify_oauth_consent_request_token(&request.request_token)?;
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let action = request.action.trim().to_ascii_lowercase();
    match action.as_str() {
        "approve" => {
            let approved_scopes = {
                match request.scope.as_deref() {
                    None => claims.scopes.clone(),
                    Some(raw_scope) => {
                        let requested = parse_scope_string(Some(raw_scope));
                        if requested.is_empty() {
                            Vec::new()
                        } else {
                            let approved: Vec<String> = requested
                                .into_iter()
                                .filter(|scope| claims.scopes.iter().any(|s| s == scope))
                                .collect();
                            if approved.is_empty() {
                                return Err((StatusCode::BAD_REQUEST, "invalid_scope").into());
                            }
                            approved
                        }
                    }
                }
            };
            let selected_resource = if let Some(expected_resource) = claims.resource.clone() {
                if let Some(provided_resource) = request.resource.as_deref() {
                    if provided_resource.trim() != expected_resource {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            "resource does not match authorization request",
                        )
                            .into());
                    }
                }
                expected_resource
            } else {
                let provided_resource = request
                    .resource
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| (StatusCode::BAD_REQUEST, "resource is required"))?;
                if !is_valid_resource_indicator(provided_resource) {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        "resource must be an absolute URI (e.g. urn:wacht:workspace:123)",
                    )
                        .into());
                }
                provided_resource.to_string()
            };
            let app_slug = EnsureUserApiAuthAppCommand::new(claims.deployment_id, request.user_id)
                .execute(&app_state)
                .await?;

            let oauth_grant_id = ensure_or_create_grant_coverage(
                &app_state,
                claims.deployment_id,
                claims.oauth_client_id,
                app_slug.clone(),
                approved_scopes.clone(),
                selected_resource.clone(),
                request.user_id,
            )
            .await?;

            let issued = IssueOAuthAuthorizationCode {
                deployment_id: claims.deployment_id,
                oauth_client_id: claims.oauth_client_id,
                oauth_grant_id,
                app_slug,
                redirect_uri: claims.redirect_uri.clone(),
                code_challenge: claims.code_challenge,
                code_challenge_method: claims.code_challenge_method,
                scopes: approved_scopes,
                resource: Some(selected_resource),
            }
            .execute(&app_state)
            .await?;

            let redirect_uri = append_oauth_redirect_params(
                claims.redirect_uri,
                &[("code", issued.code)],
                claims.state,
                Some(issuer.clone()),
            );
            Ok(Redirect::to(&redirect_uri))
        }
        "deny" => {
            let redirect_uri = append_oauth_redirect_params(
                claims.redirect_uri,
                &[("error", "access_denied".to_string())],
                claims.state,
                Some(issuer.clone()),
            );
            Ok(Redirect::to(&redirect_uri))
        }
        _ => Err((StatusCode::BAD_REQUEST, "action must be approve or deny").into()),
    }
}

pub async fn oauth_token(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthTokenRequest>,
) -> axum::response::Response {
    oauth_token_impl(app_state, headers, request)
        .await
        .into_response()
}

async fn oauth_token_impl(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthTokenRequest,
) -> Result<Json<OAuthTokenResponse>, OAuthEndpointError> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers)
        .await
        .map_err(map_token_app_error)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app).map_err(map_token_app_error)?;
    let client = authenticate_client(
        &app_state,
        &headers,
        &issuer,
        &request,
        oauth_app.id,
        "/oauth/token",
    )
        .await
        .map_err(map_token_auth_error)?;
    if !client.grant_types.iter().any(|g| g == &request.grant_type) {
        return Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "unauthorized_client",
            Some("grant_type is not allowed for this client"),
        ));
    }

    match request.grant_type.as_str() {
        "authorization_code" => {
            let code = request
                .code
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    oauth_token_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        Some("code is required"),
                    )
                })?;
            let redirect_uri = request
                .redirect_uri
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    oauth_token_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        Some("redirect_uri is required"),
                    )
                })?;
            let code_row = GetRuntimeAuthorizationCodeForExchangeQuery::new(
                oauth_app.deployment_id,
                client.id,
                hash_value(code),
            )
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?
            .ok_or_else(|| oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None))?;

            if code_row.redirect_uri != redirect_uri {
                return Err(oauth_token_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    Some("redirect_uri mismatch"),
                ));
            }
            if client.client_auth_method == "none" && code_row.pkce_code_challenge.is_none() {
                return Err(oauth_token_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    None,
                ));
            }
            verify_pkce(
                code_row.pkce_code_challenge.as_deref(),
                code_row.pkce_code_challenge_method.as_deref(),
                request.code_verifier.as_deref(),
            )
            .map_err(map_token_pkce_error)?;
            let grant_result = validate_grant_and_entitlement(
                &app_state,
                oauth_app.deployment_id,
                client.id,
                code_row.oauth_grant_id,
                code_row.app_slug.clone(),
                code_row.scopes.clone(),
                code_row.resource.clone(),
                &oauth_app.scope_definitions,
            )
            .await
            .map_err(map_token_app_error)?;
            if grant_result != GrantValidationResult::Active {
                return Err(oauth_token_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    None,
                ));
            }
            let consumed = ConsumeOAuthAuthorizationCode {
                code_id: code_row.id,
            }
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?;
            if !consumed {
                if let Some(oauth_grant_id) = code_row.oauth_grant_id {
                    let _ = RevokeOAuthTokensByGrant {
                        deployment_id: oauth_app.deployment_id,
                        oauth_client_id: client.id,
                        oauth_grant_id,
                    }
                    .execute(&app_state)
                    .await;
                }
                return Err(oauth_token_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    None,
                ));
            }
            let oauth_grant_id = code_row
                .oauth_grant_id
                .ok_or_else(|| oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None))?;

            let tokens = IssueOAuthTokenPair {
                deployment_id: oauth_app.deployment_id,
                oauth_client_id: client.id,
                oauth_grant_id,
                app_slug: code_row.app_slug,
                scopes: code_row.scopes.clone(),
                resource: code_row.resource,
            }
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?;
            enqueue_grant_last_used(
                app_state.clone(),
                oauth_app.deployment_id,
                client.id,
                oauth_grant_id,
            );

            Ok(Json(OAuthTokenResponse {
                access_token: tokens.access_token,
                token_type: "Bearer".to_string(),
                expires_in: tokens.access_expires_in,
                refresh_token: tokens.refresh_token,
                scope: code_row.scopes.join(" "),
            }))
        }
        "refresh_token" => {
            let refresh_token = request
                .refresh_token
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    oauth_token_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_request",
                        Some("refresh_token is required"),
                    )
                })?;

            let refresh_row = GetRuntimeRefreshTokenForExchangeQuery::new(
                oauth_app.deployment_id,
                client.id,
                hash_value(refresh_token),
            )
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?
            .ok_or_else(|| oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None))?;
            let now = Utc::now();
            let is_active_refresh =
                refresh_row.revoked_at.is_none() && refresh_row.expires_at > now;
            if !is_active_refresh {
                if refresh_row.replaced_by_token_id.is_some() {
                    let revoked_count = RevokeOAuthRefreshTokenFamily {
                        deployment_id: oauth_app.deployment_id,
                        oauth_client_id: client.id,
                        root_refresh_token_id: refresh_row.id,
                    }
                    .execute(&app_state)
                    .await
                    .map_err(map_token_app_error)?;
                    tracing::warn!(
                        event = "oauth.refresh_token_reuse_detected",
                        deployment_id = oauth_app.deployment_id,
                        oauth_client_id = client.id,
                        refresh_token_id = refresh_row.id,
                        revoked_refresh_tokens = revoked_count,
                        "Refresh token replay detected; refresh token family revoked",
                    );
                }
                return Err(oauth_token_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    None,
                ));
            }

            let grant_result = validate_grant_and_entitlement(
                &app_state,
                oauth_app.deployment_id,
                client.id,
                refresh_row.oauth_grant_id,
                refresh_row.app_slug.clone(),
                refresh_row.scopes.clone(),
                refresh_row.resource.clone(),
                &oauth_app.scope_definitions,
            )
            .await
            .map_err(map_token_app_error)?;
            if grant_result != GrantValidationResult::Active {
                return Err(oauth_token_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    None,
                ));
            }
            let requested_scopes = parse_scope_string(request.scope.as_deref());
            let effective_scopes = if requested_scopes.is_empty() {
                refresh_row.scopes.clone()
            } else {
                let granted_scopes: std::collections::HashSet<String> =
                    refresh_row.scopes.iter().cloned().collect();
                if !requested_scopes
                    .iter()
                    .all(|scope| granted_scopes.contains(scope))
                {
                    return Err(oauth_token_error(
                        StatusCode::BAD_REQUEST,
                        "invalid_scope",
                        Some("requested scope is not a subset of original grant"),
                    ));
                }
                requested_scopes
            };

            let revoked = RevokeOAuthRefreshTokenById {
                refresh_token_id: refresh_row.id,
            }
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?;
            if !revoked {
                return Err(oauth_token_error(
                    StatusCode::BAD_REQUEST,
                    "invalid_grant",
                    None,
                ));
            }

            let tokens = IssueOAuthTokenPair {
                deployment_id: oauth_app.deployment_id,
                oauth_client_id: client.id,
                oauth_grant_id: refresh_row.oauth_grant_id.ok_or_else(|| {
                    oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None)
                })?,
                app_slug: refresh_row.app_slug,
                scopes: effective_scopes.clone(),
                resource: refresh_row.resource.clone(),
            }
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?;

            SetOAuthRefreshTokenReplacement {
                old_refresh_token_id: refresh_row.id,
                new_refresh_token_id: tokens.refresh_token_id,
            }
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?;
            if let Some(grant_id) = refresh_row.oauth_grant_id {
                enqueue_grant_last_used(
                    app_state.clone(),
                    oauth_app.deployment_id,
                    client.id,
                    grant_id,
                );
            }

            Ok(Json(OAuthTokenResponse {
                access_token: tokens.access_token,
                token_type: "Bearer".to_string(),
                expires_in: tokens.access_expires_in,
                refresh_token: tokens.refresh_token,
                scope: effective_scopes.join(" "),
            }))
        }
        _ => Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "unsupported_grant_type",
            None,
        )),
    }
}

fn enqueue_grant_last_used(
    app_state: AppState,
    deployment_id: i64,
    oauth_client_id: i64,
    grant_id: i64,
) {
    tokio::spawn(async move {
        let _ = EnqueueOAuthGrantLastUsed {
            deployment_id,
            oauth_client_id,
            grant_id,
        }
        .execute(&app_state)
        .await;
    });
}

fn oauth_token_error(
    status: StatusCode,
    code: &str,
    description: Option<&str>,
) -> OAuthEndpointError {
    let mut headers = HeaderMap::new();
    if status == StatusCode::UNAUTHORIZED {
        let value = format!("Basic realm=\"oauth\", error=\"{}\"", code);
        if let Ok(header) = HeaderValue::from_str(&value) {
            headers.insert(WWW_AUTHENTICATE, header);
        }
    }
    (
        status,
        headers,
        Json(OAuthErrorResponse {
            error: code.to_string(),
            error_description: description.map(|v| v.to_string()),
        }),
    )
}

type OAuthEndpointError = (StatusCode, HeaderMap, Json<OAuthErrorResponse>);

fn map_token_app_error(err: AppError) -> OAuthEndpointError {
    match err {
        AppError::Unauthorized => {
            oauth_token_error(StatusCode::UNAUTHORIZED, "invalid_client", None)
        }
        AppError::BadRequest(message) | AppError::Validation(message) => {
            oauth_token_error(StatusCode::BAD_REQUEST, "invalid_request", Some(&message))
        }
        _ => oauth_token_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", None),
    }
}

fn map_token_auth_error(err: AppError) -> OAuthEndpointError {
    match err {
        AppError::Unauthorized => {
            oauth_token_error(StatusCode::UNAUTHORIZED, "invalid_client", None)
        }
        AppError::BadRequest(message) | AppError::Validation(message) => {
            oauth_token_error(StatusCode::BAD_REQUEST, "invalid_request", Some(&message))
        }
        _ => oauth_token_error(StatusCode::INTERNAL_SERVER_ERROR, "server_error", None),
    }
}

fn map_token_pkce_error(err: AppError) -> OAuthEndpointError {
    match err {
        AppError::BadRequest(message) | AppError::Validation(message) => {
            oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", Some(&message))
        }
        _ => oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None),
    }
}

pub async fn oauth_revoke(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthRevokeRequest>,
) -> axum::response::Response {
    oauth_revoke_impl(app_state, headers, request)
        .await
        .into_response()
}

async fn oauth_revoke_impl(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthRevokeRequest,
) -> Result<Json<OAuthRevokeResponse>, OAuthEndpointError> {
    let token_value = request.token.trim();
    if token_value.is_empty() {
        return Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("token is required"),
        ));
    }
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers)
        .await
        .map_err(map_token_app_error)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app).map_err(map_token_app_error)?;
    let token_req = OAuthTokenRequest {
        grant_type: String::new(),
        code: None,
        redirect_uri: None,
        scope: None,
        code_verifier: None,
        refresh_token: None,
        client_id: request.client_id,
        client_secret: request.client_secret,
        client_assertion_type: request.client_assertion_type,
        client_assertion: request.client_assertion,
    };
    let client = authenticate_client(
        &app_state,
        &headers,
        &issuer,
        &token_req,
        oauth_app.id,
        "/oauth/revoke",
    )
    .await
    .map_err(map_token_auth_error)?;

    let hash = hash_value(token_value);
    let hint = request.token_type_hint.unwrap_or_default();
    if hint != "refresh_token" {
        RevokeOAuthAccessTokenByHash {
            deployment_id: oauth_app.deployment_id,
            oauth_client_id: client.id,
            token_hash: hash.clone(),
        }
        .execute(&app_state)
        .await
        .map_err(map_token_app_error)?;
    }
    if hint != "access_token" {
        RevokeOAuthRefreshTokenByHash {
            deployment_id: oauth_app.deployment_id,
            oauth_client_id: client.id,
            token_hash: hash,
        }
        .execute(&app_state)
        .await
        .map_err(map_token_app_error)?;
    }

    Ok(Json(OAuthRevokeResponse { revoked: true }))
}

pub async fn oauth_introspect(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthIntrospectRequest>,
) -> axum::response::Response {
    oauth_introspect_impl(app_state, headers, request)
        .await
        .into_response()
}

async fn oauth_introspect_impl(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthIntrospectRequest,
) -> Result<Json<OAuthIntrospectResponse>, OAuthEndpointError> {
    let token_value = request.token.trim();
    if token_value.is_empty() {
        return Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("token is required"),
        ));
    }
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers)
        .await
        .map_err(map_token_app_error)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app).map_err(map_token_app_error)?;
    let auth_req = OAuthTokenRequest {
        grant_type: String::new(),
        code: None,
        redirect_uri: None,
        scope: None,
        code_verifier: None,
        refresh_token: None,
        client_id: request.client_id,
        client_secret: request.client_secret,
        client_assertion_type: request.client_assertion_type,
        client_assertion: request.client_assertion,
    };
    let client = authenticate_client(
        &app_state,
        &headers,
        &issuer,
        &auth_req,
        oauth_app.id,
        "/oauth/introspect",
    )
    .await
    .map_err(map_token_auth_error)?;

    let token_hash = hash_value(token_value);
    let token =
        GetRuntimeIntrospectionDataQuery::new(oauth_app.deployment_id, client.id, token_hash)
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?;

    let Some(token) = token else {
        return Ok(Json(OAuthIntrospectResponse {
            active: false,
            scope: None,
            client_id: None,
            token_type: None,
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            nbf: None,
            sub: None,
            resource: None,
        }));
    };

    if !token.active {
        return Ok(Json(OAuthIntrospectResponse {
            active: false,
            scope: None,
            client_id: None,
            token_type: None,
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            nbf: None,
            sub: None,
            resource: None,
        }));
    }
    if let Some(grant_id) = token.oauth_grant_id {
        enqueue_grant_last_used(
            app_state.clone(),
            oauth_app.deployment_id,
            client.id,
            grant_id,
        );
    }

    Ok(Json(OAuthIntrospectResponse {
        active: true,
        scope: Some(token.scopes.join(" ")),
        client_id: Some(token.client_id),
        token_type: Some("Bearer".to_string()),
        iss: Some(issuer),
        aud: token.resource.clone(),
        exp: Some(token.expires_at.timestamp()),
        iat: Some(token.issued_at.timestamp()),
        nbf: Some(token.issued_at.timestamp()),
        sub: Some(token.app_slug),
        resource: token.resource,
    }))
}

pub async fn oauth_register_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<OAuthDynamicClientRegistrationRequest>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    if !oauth_app.allow_dynamic_client_registration {
        return Err((
            StatusCode::FORBIDDEN,
            "Dynamic client registration is disabled for this OAuth app",
        )
            .into());
    }

    let grant_types = if request.grant_types.is_empty() {
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ]
    } else {
        request.grant_types.clone()
    };
    let method = request
        .token_endpoint_auth_method
        .clone()
        .unwrap_or_else(|| "client_secret_basic".to_string());

    let created = CreateOAuthClientCommand {
        deployment_id: oauth_app.deployment_id,
        oauth_app_id: oauth_app.id,
        client_auth_method: method.clone(),
        grant_types: grant_types.clone(),
        redirect_uris: request.redirect_uris.clone(),
        client_name: request.client_name.clone(),
        client_uri: request.client_uri.clone(),
        logo_uri: request.logo_uri.clone(),
        tos_uri: request.tos_uri.clone(),
        policy_uri: request.policy_uri.clone(),
        contacts: request.contacts.clone(),
        software_id: request.software_id.clone(),
        software_version: request.software_version.clone(),
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?;

    let registration_access_token = generate_registration_access_token();
    let registration_access_token_hash = hash_value(&registration_access_token);
    SetOAuthClientRegistrationAccessToken {
        oauth_app_id: oauth_app.id,
        client_id: created.client.client_id.clone(),
        registration_access_token_hash: Some(registration_access_token_hash),
    }
    .execute(&app_state)
    .await?;

    let issued_at = created.client.created_at.timestamp();
    let created_client_id = created.client.client_id.clone();
    let created_contacts = created.client.contacts_vec();
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let client_secret_expires_at = if created.client_secret.is_some() {
        Some(0)
    } else {
        None
    };
    Ok(OAuthDynamicClientRegistrationResponse {
        client_id: created_client_id.clone(),
        client_name: created.client.client_name,
        client_uri: created.client.client_uri,
        logo_uri: created.client.logo_uri,
        tos_uri: created.client.tos_uri,
        policy_uri: created.client.policy_uri,
        contacts: created_contacts,
        software_id: created.client.software_id,
        software_version: created.client.software_version,
        client_secret: created.client_secret,
        client_id_issued_at: issued_at,
        client_secret_expires_at,
        token_endpoint_auth_method: method,
        grant_types,
        redirect_uris: request.redirect_uris,
        registration_client_uri: format!("{}/oauth/register/{}", issuer, created_client_id),
        registration_access_token: Some(registration_access_token),
    }
    .into())
}

pub async fn oauth_get_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, params.client_id.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    ensure_registration_access_token(&headers, client.registration_access_token_hash.as_deref())?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;

    Ok(OAuthDynamicClientRegistrationResponse {
        client_id: client.client_id.clone(),
        client_name: client.client_name,
        client_uri: client.client_uri,
        logo_uri: client.logo_uri,
        tos_uri: client.tos_uri,
        policy_uri: client.policy_uri,
        contacts: client.contacts,
        software_id: client.software_id,
        software_version: client.software_version,
        client_secret: None,
        client_id_issued_at: client.created_at.timestamp(),
        client_secret_expires_at: client_secret_expires_at_for_method(&client.client_auth_method),
        token_endpoint_auth_method: client.client_auth_method.clone(),
        grant_types: client.grant_types.clone(),
        redirect_uris: client.redirect_uris.clone(),
        registration_client_uri: format!("{}/oauth/register/{}", issuer, client.client_id),
        registration_access_token: None,
    }
    .into())
}

pub async fn oauth_update_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
    Json(request): Json<OAuthDynamicClientUpdateRequest>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let existing =
        GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, params.client_id.clone())
            .execute(&app_state)
            .await?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;
    ensure_registration_access_token(&headers, existing.registration_access_token_hash.as_deref())?;

    let updated = UpdateOAuthClientSettings {
        oauth_app_id: oauth_app.id,
        client_id: params.client_id.clone(),
        client_name: request.client_name.clone(),
        client_uri: request.client_uri.clone(),
        logo_uri: request.logo_uri.clone(),
        tos_uri: request.tos_uri.clone(),
        policy_uri: request.policy_uri.clone(),
        contacts: request.contacts.clone(),
        software_id: request.software_id.clone(),
        software_version: request.software_version.clone(),
        client_auth_method: request.token_endpoint_auth_method.clone(),
        grant_types: request.grant_types.clone(),
        redirect_uris: request.redirect_uris.clone(),
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let updated_client_id = updated.client_id.clone();
    let updated_method = updated.client_auth_method.clone();
    let updated_grant_types = updated.grant_types_vec();
    let updated_redirect_uris = updated.redirect_uris_vec();
    let updated_contacts = updated.contacts_vec();
    Ok(OAuthDynamicClientRegistrationResponse {
        client_id: updated_client_id.clone(),
        client_name: updated.client_name,
        client_uri: updated.client_uri,
        logo_uri: updated.logo_uri,
        tos_uri: updated.tos_uri,
        policy_uri: updated.policy_uri,
        contacts: updated_contacts,
        software_id: updated.software_id,
        software_version: updated.software_version,
        client_secret: None,
        client_id_issued_at: updated.created_at.timestamp(),
        client_secret_expires_at: client_secret_expires_at_for_method(&updated_method),
        token_endpoint_auth_method: updated_method,
        grant_types: updated_grant_types,
        redirect_uris: updated_redirect_uris,
        registration_client_uri: format!("{}/oauth/register/{}", issuer, updated_client_id),
        registration_access_token: None,
    }
    .into())
}

pub async fn oauth_delete_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
) -> ApiResult<()> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let existing =
        GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, params.client_id.clone())
            .execute(&app_state)
            .await?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;
    ensure_registration_access_token(&headers, existing.registration_access_token_hash.as_deref())?;

    let _ = DeactivateOAuthClient {
        oauth_app_id: oauth_app.id,
        client_id: params.client_id,
    }
    .execute(&app_state)
    .await?;

    Ok((StatusCode::NO_CONTENT, ()).into())
}

async fn resolve_oauth_app_from_host(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<queries::RuntimeOAuthAppData, AppError> {
    let host = resolve_host(headers)
        .and_then(normalize_fqdn_host)
        .ok_or_else(|| AppError::NotFound("OAuth app not found for host".to_string()))?;

    ResolveOAuthAppByFqdnQuery::new(host.to_string())
        .execute(app_state)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth app not found for host".to_string()))
}

async fn authenticate_client(
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
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app_id, client_id)
        .execute(app_state)
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

async fn validate_grant_and_entitlement(
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
        ResolveRuntimeOAuthGrantQuery::by_grant_id(deployment_id, oauth_client_id, grant_id)
            .execute(app_state)
            .await?
    } else {
        ResolveRuntimeOAuthGrantQuery::by_scope_match(
            deployment_id,
            oauth_client_id,
            app_slug.clone(),
            scopes.clone(),
            resource.clone(),
        )
        .execute(app_state)
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

    let user_id = GetRuntimeApiAuthUserIdByAppSlugQuery::new(deployment_id, app_slug)
        .execute(app_state)
        .await?;
    let Some(user_id) = user_id else {
        return Ok(GrantValidationResult::MissingOrInsufficient);
    };

    let entitled = ValidateRuntimeResourceEntitlementQuery::new(
        deployment_id,
        user_id,
        resource,
        required_permissions,
    )
    .execute(app_state)
    .await?;
    if entitled {
        Ok(GrantValidationResult::Active)
    } else {
        Ok(GrantValidationResult::MissingOrInsufficient)
    }
}

async fn ensure_or_create_grant_coverage(
    app_state: &AppState,
    deployment_id: i64,
    oauth_client_id: i64,
    app_slug: String,
    scopes: Vec<String>,
    resource: String,
    user_id: i64,
) -> Result<i64, AppError> {
    let resolved = ResolveRuntimeOAuthGrantQuery::by_scope_match(
        deployment_id,
        oauth_client_id,
        app_slug.clone(),
        scopes.clone(),
        Some(resource.clone()),
    )
    .execute(app_state)
    .await?;
    if let Some(grant_id) = resolved.active_grant_id {
        return Ok(grant_id);
    }
    if resolved.revoked {
        return Err(AppError::Forbidden(
            "Grant is revoked for requested scopes/resource".to_string(),
        ));
    }

    let created = CreateOAuthClientGrantCommand {
        deployment_id,
        api_auth_app_slug: app_slug,
        oauth_client_id,
        resource,
        scopes,
        granted_by_user_id: Some(user_id),
        expires_at: None,
    }
    .execute(app_state)
    .await?;
    Ok(created.id)
}

fn validate_secret_hash(stored_hash: Option<&str>, provided_secret: &str) -> Result<(), AppError> {
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

fn validate_assertion_type(assertion_type: Option<&str>) -> Result<(), AppError> {
    if assertion_type.unwrap_or_default()
        != "urn:ietf:params:oauth:client-assertion-type:jwt-bearer"
    {
        return Err(AppError::Unauthorized);
    }
    Ok(())
}

fn validate_assertion_claims(
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

fn audience_matches(aud: &serde_json::Value, expected: &str) -> bool {
    match aud {
        serde_json::Value::String(s) => s == expected,
        serde_json::Value::Array(values) => values
            .iter()
            .any(|value| value.as_str().is_some_and(|s| s == expected)),
        _ => false,
    }
}

async fn enforce_assertion_replay_protection(
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

fn extract_basic_credentials(
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

fn verify_pkce(
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

fn is_valid_resource_indicator(resource: &str) -> bool {
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

fn required_permissions_for_resource(
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

fn parse_scope_string(scope: Option<&str>) -> Vec<String> {
    scope
        .unwrap_or_default()
        .split(' ')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn hash_value(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn resolve_issuer_from_oauth_app(
    oauth_app: &queries::RuntimeOAuthAppData,
) -> Result<String, AppError> {
    let fqdn = oauth_app.fqdn.trim();
    if fqdn.is_empty() {
        return Err(AppError::BadRequest("oauth app fqdn is required".to_string()));
    }
    if fqdn.contains("://") || fqdn.contains(':') || fqdn.contains('/') {
        return Err(AppError::BadRequest(
            "oauth app fqdn must be a bare host without scheme, port, or path".to_string(),
        ));
    }
    Ok(format!("https://{}", fqdn))
}

fn resolve_host(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("x-forwarded-host")
        .and_then(|v| v.to_str().ok())
        .or_else(|| headers.get("host").and_then(|v| v.to_str().ok()))
        .and_then(|v| v.split(',').next())
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn normalize_fqdn_host(host: &str) -> Option<&str> {
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

fn ensure_registration_access_token(
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

fn generate_registration_access_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!(
        "orat_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

fn generate_prefixed_token(prefix: &str, bytes_len: usize) -> String {
    let mut bytes = vec![0u8; bytes_len];
    rand::rng().fill_bytes(&mut bytes);
    format!(
        "{}_{}",
        prefix,
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )
}

fn client_secret_expires_at_for_method(client_auth_method: &str) -> Option<i64> {
    match client_auth_method {
        "none" | "private_key_jwt" => None,
        _ => Some(0),
    }
}

fn oauth_consent_request_secret() -> Result<String, AppError> {
    derive_shared_secret("oauth-consent-request-v1")
}

fn sign_oauth_consent_request_token(
    claims: &OAuthConsentRequestTokenClaims,
) -> Result<String, AppError> {
    let payload_json =
        serde_json::to_vec(claims).map_err(|e| AppError::Serialization(e.to_string()))?;
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload_json);
    let signature = sign_payload(payload.as_bytes())?;
    Ok(format!("ocrt.{}.{}", payload, signature))
}

fn verify_oauth_consent_request_token(
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

fn sign_payload(payload: &[u8]) -> Result<String, AppError> {
    type HmacSha256 = Hmac<Sha256>;
    let secret = oauth_consent_request_secret()?;
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .map_err(|e| AppError::Internal(e.to_string()))?;
    mac.update(payload);
    let signature = mac.finalize().into_bytes();
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature))
}

fn append_oauth_redirect_params(
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
