//! Thin Axum handlers for OIDC extension endpoints. Business logic lives in
//! `crate::application::oauth_runtime::oidc`.

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::WWW_AUTHENTICATE},
    response::{IntoResponse, Redirect, Response},
};
use common::state::AppState;
use dto::json::oauth_runtime::{JwksResponse, OAuthLogoutRequest, OpenIdConfigurationResponse};

use crate::application::{oauth_runtime as oauth_runtime_app, response::ApiResult};

pub async fn openid_configuration(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<OpenIdConfigurationResponse> {
    let response = oauth_runtime_app::openid_configuration(&app_state, &headers).await?;
    Ok(response.into())
}

pub async fn jwks(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<JwksResponse> {
    let response = oauth_runtime_app::jwks(&app_state, &headers).await?;
    Ok(response.into())
}

/// RFC 6750 / OIDC Core 5.3.3 compliant userinfo handler. On failure we emit
/// a `WWW-Authenticate: Bearer ...` header with the standard error codes
/// (`invalid_token`, `insufficient_scope`, `invalid_request`) so RP libraries
/// can recognise the error without parsing a custom JSON envelope.
pub async fn userinfo(State(app_state): State<AppState>, headers: HeaderMap) -> Response {
    match oauth_runtime_app::userinfo(&app_state, &headers).await {
        Ok(body) => Json(body).into_response(),
        Err(err) => bearer_error_response(err),
    }
}

fn bearer_error_response(err: oauth_runtime_app::UserInfoError) -> Response {
    use oauth_runtime_app::UserInfoError;
    let (status, www_authenticate) = match &err {
        UserInfoError::MissingToken => (
            StatusCode::UNAUTHORIZED,
            "Bearer realm=\"wacht\"".to_string(),
        ),
        UserInfoError::InvalidToken(desc) => (
            StatusCode::UNAUTHORIZED,
            format!(
                "Bearer realm=\"wacht\", error=\"invalid_token\", error_description=\"{}\"",
                desc
            ),
        ),
        UserInfoError::InsufficientScope { required, message } => (
            StatusCode::FORBIDDEN,
            format!(
                "Bearer realm=\"wacht\", error=\"insufficient_scope\", scope=\"{}\", error_description=\"{}\"",
                required, message
            ),
        ),
        UserInfoError::InvalidRequest(desc) => (
            StatusCode::BAD_REQUEST,
            format!(
                "Bearer realm=\"wacht\", error=\"invalid_request\", error_description=\"{}\"",
                desc
            ),
        ),
        UserInfoError::Internal(inner) => {
            return inner.clone().into_response();
        }
    };
    let mut response = status.into_response();
    if let Ok(value) = HeaderValue::from_str(&www_authenticate) {
        response.headers_mut().insert(WWW_AUTHENTICATE, value);
    }
    response
}

pub async fn oauth_logout(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(request): Query<OAuthLogoutRequest>,
) -> Result<Redirect, crate::application::response::ApiErrorResponse> {
    oauth_runtime_app::oauth_logout(&app_state, &headers, request).await
}

/// OIDC RP-Initiated Logout requires the end-session endpoint to accept both
/// GET and POST. With POST the parameters arrive form-encoded in the body.
pub async fn oauth_logout_post(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Form(request): axum::extract::Form<OAuthLogoutRequest>,
) -> Result<Redirect, crate::application::response::ApiErrorResponse> {
    oauth_runtime_app::oauth_logout(&app_state, &headers, request).await
}
