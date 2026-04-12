use axum::{
    Json,
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use common::state::AppState;
use dto::json::oauth_runtime::{
    OAuthDynamicClientRegistrationRequest, OAuthDynamicClientRegistrationResponse,
    OAuthDynamicClientUpdateRequest, OAuthIntrospectRequest, OAuthRegisterPathParams,
    OAuthRevokeRequest,
};

use crate::application::{oauth_runtime as oauth_runtime_app, response::ApiResult};

pub async fn oauth_revoke(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthRevokeRequest>,
) -> axum::response::Response {
    oauth_runtime_app::oauth_revoke(app_state, headers, request)
        .await
        .into_response()
}

pub async fn oauth_introspect(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthIntrospectRequest>,
) -> axum::response::Response {
    oauth_runtime_app::oauth_introspect(app_state, headers, request)
        .await
        .into_response()
}

pub async fn oauth_register_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<OAuthDynamicClientRegistrationRequest>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let response = oauth_runtime_app::oauth_register_client(&app_state, &headers, request).await?;
    Ok(response.into())
}

pub async fn oauth_get_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let response =
        oauth_runtime_app::oauth_get_registered_client(&app_state, &headers, params).await?;
    Ok(response.into())
}

pub async fn oauth_update_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
    Json(request): Json<OAuthDynamicClientUpdateRequest>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let response =
        oauth_runtime_app::oauth_update_registered_client(&app_state, &headers, params, request)
            .await?;
    Ok(response.into())
}

pub async fn oauth_delete_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
) -> ApiResult<()> {
    oauth_runtime_app::oauth_delete_registered_client(&app_state, &headers, params).await?;
    Ok((StatusCode::NO_CONTENT, ()).into())
}
