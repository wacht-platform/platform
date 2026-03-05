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
use queries::{
    Query as QueryTrait,
    oauth::{GetOAuthAppBySlugQuery, GetOAuthClientByIdQuery, ListOAuthClientsByOAuthAppQuery},
};

use super::mappers::map_oauth_client_response;
use super::types::{OAuthAppPathParams, OAuthClientPathParams};

pub(crate) async fn list_oauth_clients(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthAppPathParams>,
) -> ApiResult<ListOAuthClientsResponse> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

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
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

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

    let grant_types = created.client.grant_types_vec();
    let redirect_uris = created.client.redirect_uris_vec();
    let contacts = created.client.contacts_vec();

    Ok(OAuthClientResponse {
        id: created.client.id,
        oauth_app_id: created.client.oauth_app_id,
        client_id: created.client.client_id,
        client_auth_method: created.client.client_auth_method,
        grant_types,
        redirect_uris,
        client_name: created.client.client_name,
        client_uri: created.client.client_uri,
        logo_uri: created.client.logo_uri,
        tos_uri: created.client.tos_uri,
        policy_uri: created.client.policy_uri,
        contacts,
        software_id: created.client.software_id,
        software_version: created.client.software_version,
        token_endpoint_auth_signing_alg: created.client.token_endpoint_auth_signing_alg,
        jwks_uri: created.client.jwks_uri,
        jwks: created.client.jwks,
        public_key_pem: created.client.public_key_pem,
        is_active: created.client.is_active,
        created_at: created.client.created_at,
        updated_at: created.client.updated_at,
        client_secret: created.client_secret,
    }
    .into())
}

pub(crate) async fn update_oauth_client(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OAuthClientPathParams>,
    Json(request): Json<UpdateOAuthClientRequest>,
) -> ApiResult<OAuthClientResponse> {
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

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
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

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
    let oauth_app = GetOAuthAppBySlugQuery::new(deployment_id, params.oauth_app_slug)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth app not found"))?;

    let client = GetOAuthClientByIdQuery::new(deployment_id, oauth_app.id, params.oauth_client_id)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    let client_secret = RotateOAuthClientSecret {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found or inactive"))?;

    Ok(RotateOAuthClientSecretResponse { client_secret }.into())
}
