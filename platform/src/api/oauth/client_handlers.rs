use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use commands::{
    Command,
    oauth::{
        CreateOAuthClientCommand, DeactivateOAuthClient, RotateOAuthClientSecret,
        UpdateOAuthClientSettings,
    },
};
use common::state::AppState;
use dto::json::api_key::{
    CreateOAuthClientRequest, ListOAuthClientsResponse, OAuthClientResponse,
    RotateOAuthClientSecretResponse, UpdateOAuthClientRequest,
};
use queries::{Query as QueryTrait, oauth::ListOAuthClientsByOAuthAppQuery};

use super::helpers::{get_oauth_app_by_slug, get_oauth_client_by_id};
use super::mappers::{map_oauth_client_response, map_oauth_client_response_with_secret};
use super::types::{OAuthAppPathParams, OAuthClientPathParams};

pub(crate) async fn list_oauth_clients(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<ListOAuthClientsResponse> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;

    let clients = ListOAuthClientsByOAuthAppQuery::new(deployment_id, oauth_app.id)
        .execute(&app_state)
        .await?
        .into_iter()
        .map(map_oauth_client_response)
        .collect();

    Ok(ListOAuthClientsResponse { clients }.into())
}

pub(crate) async fn create_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
    Json(request): Json<CreateOAuthClientRequest>,
) -> ApiResult<OAuthClientResponse> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;

    let created = CreateOAuthClientCommand {
        deployment_id,
        oauth_app_id: oauth_app.id,
        client_auth_method: request.client_auth_method,
        grant_types: request.grant_types,
        redirect_uris: request.redirect_uris,
        client_name: request.client_name,
        client_uri: request.client_uri,
        logo_uri: request.logo_uri,
        tos_uri: request.tos_uri,
        policy_uri: request.policy_uri,
        contacts: request.contacts,
        software_id: request.software_id,
        software_version: request.software_version,
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?;

    Ok(map_oauth_client_response_with_secret(created.client, created.client_secret).into())
}

pub(crate) async fn update_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
    Json(request): Json<UpdateOAuthClientRequest>,
) -> ApiResult<OAuthClientResponse> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;
    let client = get_oauth_client_by_id(
        &app_state,
        deployment_id,
        oauth_app.id,
        params.oauth_client_id,
    )
    .await?;

    let updated = UpdateOAuthClientSettings {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
        client_auth_method: request.client_auth_method,
        grant_types: request.grant_types,
        redirect_uris: request.redirect_uris,
        client_name: request.client_name,
        client_uri: request.client_uri,
        logo_uri: request.logo_uri,
        tos_uri: request.tos_uri,
        policy_uri: request.policy_uri,
        contacts: request.contacts,
        software_id: request.software_id,
        software_version: request.software_version,
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found or inactive"))?;

    Ok(map_oauth_client_response(updated).into())
}

pub(crate) async fn deactivate_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<()> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;
    let client = get_oauth_client_by_id(
        &app_state,
        deployment_id,
        oauth_app.id,
        params.oauth_client_id,
    )
    .await?;

    let updated = DeactivateOAuthClient {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
    }
    .execute(&app_state)
    .await?;

    if !updated {
        return Err((
            StatusCode::NOT_FOUND,
            "OAuth client not found or already inactive",
        )
            .into());
    }

    Ok((StatusCode::NO_CONTENT, ()).into())
}

pub(crate) async fn rotate_oauth_client_secret(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
) -> ApiResult<RotateOAuthClientSecretResponse> {
    let oauth_app = get_oauth_app_by_slug(&app_state, deployment_id, params.oauth_app_slug).await?;
    let client = get_oauth_client_by_id(
        &app_state,
        deployment_id,
        oauth_app.id,
        params.oauth_client_id,
    )
    .await?;

    let client_secret = RotateOAuthClientSecret {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found or inactive"))?;

    Ok(RotateOAuthClientSecretResponse { client_secret }.into())
}
