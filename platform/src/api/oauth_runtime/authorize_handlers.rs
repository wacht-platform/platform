use axum::{
    extract::{Form, Query, State},
    http::HeaderMap,
    response::Redirect,
};
use common::state::AppState;
use dto::json::oauth_runtime::{
    OAuthAuthorizeRequest, OAuthConsentSubmitRequest, OAuthProtectedResourceMetadataResponse,
    OAuthServerMetadataResponse,
};

use crate::application::{oauth_runtime as oauth_runtime_app, response::ApiResult};

pub async fn oauth_server_metadata(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<OAuthServerMetadataResponse> {
    let response = oauth_runtime_app::oauth_server_metadata(&app_state, &headers).await?;
    Ok(response.into())
}

pub async fn oauth_protected_resource_metadata(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<OAuthProtectedResourceMetadataResponse> {
    let response =
        oauth_runtime_app::oauth_protected_resource_metadata(&app_state, &headers).await?;
    Ok(response.into())
}

pub async fn oauth_authorize_get(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Query(request): Query<OAuthAuthorizeRequest>,
) -> Result<Redirect, crate::application::response::ApiErrorResponse> {
    oauth_runtime_app::oauth_authorize_get(&app_state, &headers, request).await
}

/// OIDC Core §3.1.2.1 requires the Authorization Endpoint to accept both GET
/// and POST. With POST the parameters arrive in an `application/x-www-form-urlencoded`
/// body. The handler logic is otherwise identical.
pub async fn oauth_authorize_post(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthAuthorizeRequest>,
) -> Result<Redirect, crate::application::response::ApiErrorResponse> {
    oauth_runtime_app::oauth_authorize_get(&app_state, &headers, request).await
}

pub async fn oauth_consent_submit(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthConsentSubmitRequest>,
) -> Result<Redirect, crate::application::response::ApiErrorResponse> {
    oauth_runtime_app::oauth_consent_submit(&app_state, &headers, request).await
}
