use commands::oauth::{
    CreateOAuthClientCommand, DeactivateOAuthClient, RotateOAuthClientSecret,
    UpdateOAuthClientSettings,
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::api_key::{
    CreateOAuthClientRequest, ListOAuthClientsResponse, OAuthClientResponse,
    RotateOAuthClientSecretResponse, UpdateOAuthClientRequest,
};
use models::error::AppError;
use queries::oauth::ListOAuthClientsByOAuthAppQuery;

use super::oauth_shared::{
    get_oauth_app_by_slug, get_oauth_client_by_id, map_oauth_client_response,
    map_oauth_client_response_with_secret,
};
use common::deps;

pub async fn list_oauth_clients(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
) -> Result<ListOAuthClientsResponse, AppError> {
    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);

    let clients = ListOAuthClientsByOAuthAppQuery::new(deployment_id, oauth_app.id)
        .execute_with_db(reader)
        .await?
        .into_iter()
        .map(map_oauth_client_response)
        .collect();

    Ok(ListOAuthClientsResponse { clients })
}

pub async fn create_oauth_client(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    request: CreateOAuthClientRequest,
) -> Result<OAuthClientResponse, AppError> {
    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let deps = deps::from_app(app_state).db().enc();

    // OIDC: only RSA-family signing algs are supported because per-app keys
    // are RSA. Fail at write time, not at first id_token mint.
    if let Some(alg) = request.id_token_signing_alg.as_deref() {
        crate::api::oauth_runtime::helpers::validate_id_token_signing_alg(alg)?;
    }
    if let Some(fmt) = request.access_token_format.as_deref() {
        if fmt != "opaque" && fmt != "jwt" {
            return Err(AppError::BadRequest(
                "access_token_format must be 'opaque' or 'jwt'".to_string(),
            ));
        }
    }
    if let Some(ttl) = request.access_token_ttl_seconds {
        if !(60..=86400).contains(&ttl) {
            return Err(AppError::BadRequest(
                "access_token_ttl_seconds must be between 60 and 86400".to_string(),
            ));
        }
    }

    let created = CreateOAuthClientCommand {
        client_record_id: Some(app_state.sf.next_id()? as i64),
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
        post_logout_redirect_uris: request.post_logout_redirect_uris,
        id_token_signing_alg: request.id_token_signing_alg,
        access_token_format: request.access_token_format,
        access_token_ttl_seconds: request.access_token_ttl_seconds,
        skip_consent: request.skip_consent,
    }
    .execute_with_deps(&deps)
    .await?;

    Ok(map_oauth_client_response_with_secret(
        created.client,
        created.client_secret,
    ))
}

pub async fn update_oauth_client(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    oauth_client_id: i64,
    request: UpdateOAuthClientRequest,
) -> Result<OAuthClientResponse, AppError> {
    let writer = app_state.db_router.writer();
    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let client =
        get_oauth_client_by_id(app_state, deployment_id, oauth_app.id, oauth_client_id).await?;

    if let Some(alg) = request.id_token_signing_alg.as_deref() {
        crate::api::oauth_runtime::helpers::validate_id_token_signing_alg(alg)?;
    }
    if let Some(fmt) = request.access_token_format.as_deref() {
        if fmt != "opaque" && fmt != "jwt" {
            return Err(AppError::BadRequest(
                "access_token_format must be 'opaque' or 'jwt'".to_string(),
            ));
        }
    }
    if let Some(ttl) = request.access_token_ttl_seconds {
        if !(60..=86400).contains(&ttl) {
            return Err(AppError::BadRequest(
                "access_token_ttl_seconds must be between 60 and 86400".to_string(),
            ));
        }
    }

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
        post_logout_redirect_uris: request.post_logout_redirect_uris,
        id_token_signing_alg: request.id_token_signing_alg,
        access_token_format: request.access_token_format,
        access_token_ttl_seconds: request.access_token_ttl_seconds,
        skip_consent: request.skip_consent,
    }
    .execute_with_db(writer)
    .await?
    .ok_or_else(|| AppError::NotFound("OAuth client not found or inactive".to_string()))?;

    Ok(map_oauth_client_response(updated))
}

pub async fn deactivate_oauth_client(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    oauth_client_id: i64,
) -> Result<(), AppError> {
    let writer = app_state.db_router.writer();
    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let client =
        get_oauth_client_by_id(app_state, deployment_id, oauth_app.id, oauth_client_id).await?;

    let updated = DeactivateOAuthClient {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
    }
    .execute_with_db(writer)
    .await?;

    if !updated {
        return Err(AppError::NotFound(
            "OAuth client not found or already inactive".to_string(),
        ));
    }

    Ok(())
}

pub async fn rotate_oauth_client_secret(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
    oauth_client_id: i64,
) -> Result<RotateOAuthClientSecretResponse, AppError> {
    let oauth_app = get_oauth_app_by_slug(app_state, deployment_id, oauth_app_slug).await?;
    let client =
        get_oauth_client_by_id(app_state, deployment_id, oauth_app.id, oauth_client_id).await?;
    let deps = deps::from_app(app_state).db().enc();

    let client_secret = RotateOAuthClientSecret {
        oauth_app_id: oauth_app.id,
        client_id: client.client_id,
    }
    .execute_with_deps(&deps)
    .await?
    .ok_or_else(|| AppError::NotFound("OAuth client not found or inactive".to_string()))?;

    Ok(RotateOAuthClientSecretResponse { client_secret })
}
