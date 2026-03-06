use common::state::AppState;
use dto::json::api_key::{OAuthAppResponse, OAuthClientResponse};
use models::error::AppError;
use queries::{
    Query as QueryTrait,
    oauth::{GetOAuthAppBySlugQuery, GetOAuthClientByIdQuery, OAuthAppData, OAuthClientData},
};

pub async fn get_oauth_app_by_slug(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_slug: String,
) -> Result<OAuthAppData, AppError> {
    GetOAuthAppBySlugQuery::new(deployment_id, oauth_app_slug)
        .execute(app_state)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth app not found".to_string()))
}

pub async fn get_oauth_client_by_id(
    app_state: &AppState,
    deployment_id: i64,
    oauth_app_id: i64,
    oauth_client_id: i64,
) -> Result<OAuthClientData, AppError> {
    GetOAuthClientByIdQuery::new(deployment_id, oauth_app_id, oauth_client_id)
        .execute(app_state)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth client not found".to_string()))
}

pub fn map_oauth_client_response(c: OAuthClientData) -> OAuthClientResponse {
    map_oauth_client_response_with_secret(c, None)
}

pub fn map_oauth_client_response_with_secret(
    c: OAuthClientData,
    client_secret: Option<String>,
) -> OAuthClientResponse {
    let grant_types = c.grant_types_vec();
    let redirect_uris = c.redirect_uris_vec();
    let contacts = c.contacts_vec();

    OAuthClientResponse {
        id: c.id,
        oauth_app_id: c.oauth_app_id,
        client_id: c.client_id,
        client_auth_method: c.client_auth_method,
        grant_types,
        redirect_uris,
        client_name: c.client_name,
        client_uri: c.client_uri,
        logo_uri: c.logo_uri,
        tos_uri: c.tos_uri,
        policy_uri: c.policy_uri,
        contacts,
        software_id: c.software_id,
        software_version: c.software_version,
        token_endpoint_auth_signing_alg: c.token_endpoint_auth_signing_alg,
        jwks_uri: c.jwks_uri,
        jwks: c.jwks,
        public_key_pem: c.public_key_pem,
        is_active: c.is_active,
        created_at: c.created_at,
        updated_at: c.updated_at,
        client_secret,
    }
}

pub fn map_oauth_app_response(a: OAuthAppData) -> OAuthAppResponse {
    let supported_scopes = a.supported_scopes_vec();
    let scope_definitions = a.scope_definitions_vec();

    OAuthAppResponse {
        id: a.id,
        slug: a.slug,
        name: a.name,
        description: a.description,
        logo_url: a.logo_url,
        fqdn: a.fqdn,
        supported_scopes,
        scope_definitions,
        allow_dynamic_client_registration: a.allow_dynamic_client_registration,
        is_active: a.is_active,
        created_at: a.created_at,
        updated_at: a.updated_at,
    }
}
