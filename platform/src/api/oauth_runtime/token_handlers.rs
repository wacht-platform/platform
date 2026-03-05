use axum::{
    Json,
    extract::{Form, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::WWW_AUTHENTICATE},
    response::IntoResponse,
};
use chrono::Utc;
use commands::{
    Command, ConsumeOAuthAuthorizationCode, EnqueueOAuthGrantLastUsed, IssueOAuthTokenPair,
    RevokeOAuthRefreshTokenById, RevokeOAuthRefreshTokenFamily, RevokeOAuthTokensByGrant,
    SetOAuthRefreshTokenReplacement,
};
use common::{error::AppError, state::AppState};
use dto::json::oauth_runtime::{OAuthErrorResponse, OAuthTokenRequest, OAuthTokenResponse};
use queries::Query as QueryTrait;
use queries::{
    GetRuntimeAuthorizationCodeForExchangeQuery, GetRuntimeRefreshTokenForExchangeQuery,
};

use super::helpers::{
    authenticate_client, hash_value, parse_scope_string, resolve_issuer_from_oauth_app,
    resolve_oauth_app_from_host, validate_grant_and_entitlement, verify_pkce,
};
use super::types::GrantValidationResult;

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
                code_row.granted_resource.clone(),
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
                granted_resource: code_row.granted_resource,
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
                refresh_row.granted_resource.clone(),
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
                granted_resource: refresh_row.granted_resource.clone(),
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

pub(super) fn enqueue_grant_last_used(
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

pub(super) fn oauth_token_error(
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

pub(super) type OAuthEndpointError = (StatusCode, HeaderMap, Json<OAuthErrorResponse>);

pub(super) fn map_token_app_error(err: AppError) -> OAuthEndpointError {
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

pub(super) fn map_token_auth_error(err: AppError) -> OAuthEndpointError {
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

pub(super) fn map_token_pkce_error(err: AppError) -> OAuthEndpointError {
    match err {
        AppError::BadRequest(message) | AppError::Validation(message) => {
            oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", Some(&message))
        }
        _ => oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None),
    }
}
