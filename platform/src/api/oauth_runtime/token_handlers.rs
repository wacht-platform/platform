use axum::{
    Json,
    extract::{Form, State},
    http::{HeaderMap, HeaderValue, StatusCode, header::WWW_AUTHENTICATE},
    response::IntoResponse,
};
use common::{error::AppError, state::AppState};
use dto::json::oauth_runtime::{OAuthErrorResponse, OAuthTokenRequest};

use crate::application::oauth_runtime as oauth_runtime_app;

pub async fn oauth_token(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthTokenRequest>,
) -> axum::response::Response {
    oauth_runtime_app::oauth_token(app_state, headers, request)
        .await
        .into_response()
}

pub(crate) fn oauth_token_error(
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

pub(crate) type OAuthEndpointError = (StatusCode, HeaderMap, Json<OAuthErrorResponse>);

pub(crate) fn map_token_app_error(err: AppError) -> OAuthEndpointError {
    map_token_common_error(err)
}

pub(crate) fn map_token_auth_error(err: AppError) -> OAuthEndpointError {
    map_token_common_error(err)
}

fn map_token_common_error(err: AppError) -> OAuthEndpointError {
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

pub(crate) fn map_token_pkce_error(err: AppError) -> OAuthEndpointError {
    match err {
        AppError::BadRequest(message) | AppError::Validation(message) => {
            oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", Some(&message))
        }
        _ => oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None),
    }
}
