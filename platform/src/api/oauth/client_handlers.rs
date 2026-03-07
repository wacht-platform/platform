use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::application::{oauth_client as oauth_client_app, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::{
    CreateOAuthClientRequest, ListOAuthClientsResponse, OAuthClientResponse,
    RotateOAuthClientSecretResponse, UpdateOAuthClientRequest,
};

use super::types::{OAuthAppPathParams, OAuthClientPathParams};

pub(crate) async fn list_oauth_clients(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<ListOAuthClientsResponse> {
    let clients = oauth_client_app::list_oauth_clients(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
    )
    .await?;
    Ok(clients.into())
}

pub(crate) async fn create_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
    Json(request): Json<CreateOAuthClientRequest>,
) -> ApiResult<OAuthClientResponse> {
    let client = oauth_client_app::create_oauth_client(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        request,
    )
    .await?;

    Ok(client.into())
}

pub(crate) async fn update_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
    Json(request): Json<UpdateOAuthClientRequest>,
) -> ApiResult<OAuthClientResponse> {
    let client = oauth_client_app::update_oauth_client(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.oauth_client_id,
        request,
    )
    .await?;

    Ok(client.into())
}

pub(crate) async fn deactivate_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<()> {
    oauth_client_app::deactivate_oauth_client(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.oauth_client_id,
    )
    .await?;

    Ok((StatusCode::NO_CONTENT, ()).into())
}

pub(crate) async fn rotate_oauth_client_secret(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<RotateOAuthClientSecretResponse> {
    let response = oauth_client_app::rotate_oauth_client_secret(
        &app_state,
        deployment_id,
        params.oauth_app_slug,
        params.oauth_client_id,
    )
    .await?;

    Ok(response.into())
}
