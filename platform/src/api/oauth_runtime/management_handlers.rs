use axum::{
    Json,
    extract::{Form, Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use commands::{
    Command, CreateOAuthClientCommand, DeactivateOAuthClient, RevokeOAuthAccessTokenByHash,
    RevokeOAuthRefreshTokenByHash, SetOAuthClientRegistrationAccessToken,
    UpdateOAuthClientSettings,
};
use common::state::AppState;
use dto::json::oauth_runtime::{
    OAuthDynamicClientRegistrationRequest, OAuthDynamicClientRegistrationResponse,
    OAuthDynamicClientUpdateRequest, OAuthIntrospectRequest, OAuthIntrospectResponse,
    OAuthRegisterPathParams, OAuthRevokeRequest, OAuthRevokeResponse, OAuthTokenRequest,
};
use queries::Query as QueryTrait;
use queries::{GetRuntimeIntrospectionDataQuery, GetRuntimeOAuthClientByClientIdQuery};

use crate::application::response::ApiResult;

use super::helpers::{
    authenticate_client, client_secret_expires_at_for_method, ensure_registration_access_token,
    generate_registration_access_token, hash_value, resolve_issuer_from_oauth_app,
    resolve_oauth_app_from_host,
};
use super::token_handlers::{
    OAuthEndpointError, enqueue_grant_last_used, map_token_app_error, map_token_auth_error,
    oauth_token_error,
};

pub async fn oauth_revoke(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthRevokeRequest>,
) -> axum::response::Response {
    oauth_revoke_impl(app_state, headers, request)
        .await
        .into_response()
}

async fn oauth_revoke_impl(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthRevokeRequest,
) -> Result<Json<OAuthRevokeResponse>, OAuthEndpointError> {
    let token_value = request.token.trim();
    if token_value.is_empty() {
        return Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("token is required"),
        ));
    }
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers)
        .await
        .map_err(map_token_app_error)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app).map_err(map_token_app_error)?;
    let token_req = OAuthTokenRequest {
        grant_type: String::new(),
        code: None,
        redirect_uri: None,
        scope: None,
        code_verifier: None,
        refresh_token: None,
        client_id: request.client_id,
        client_secret: request.client_secret,
        client_assertion_type: request.client_assertion_type,
        client_assertion: request.client_assertion,
    };
    let client = authenticate_client(
        &app_state,
        &headers,
        &issuer,
        &token_req,
        oauth_app.id,
        "/oauth/revoke",
    )
    .await
    .map_err(map_token_auth_error)?;

    let hash = hash_value(token_value);
    let hint = request.token_type_hint.unwrap_or_default();
    if hint != "refresh_token" {
        RevokeOAuthAccessTokenByHash {
            deployment_id: oauth_app.deployment_id,
            oauth_client_id: client.id,
            token_hash: hash.clone(),
        }
        .execute(&app_state)
        .await
        .map_err(map_token_app_error)?;
    }
    if hint != "access_token" {
        RevokeOAuthRefreshTokenByHash {
            deployment_id: oauth_app.deployment_id,
            oauth_client_id: client.id,
            token_hash: hash,
        }
        .execute(&app_state)
        .await
        .map_err(map_token_app_error)?;
    }

    Ok(Json(OAuthRevokeResponse { revoked: true }))
}

pub async fn oauth_introspect(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Form(request): Form<OAuthIntrospectRequest>,
) -> axum::response::Response {
    oauth_introspect_impl(app_state, headers, request)
        .await
        .into_response()
}

async fn oauth_introspect_impl(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthIntrospectRequest,
) -> Result<Json<OAuthIntrospectResponse>, OAuthEndpointError> {
    let token_value = request.token.trim();
    if token_value.is_empty() {
        return Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("token is required"),
        ));
    }
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers)
        .await
        .map_err(map_token_app_error)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app).map_err(map_token_app_error)?;
    let auth_req = OAuthTokenRequest {
        grant_type: String::new(),
        code: None,
        redirect_uri: None,
        scope: None,
        code_verifier: None,
        refresh_token: None,
        client_id: request.client_id,
        client_secret: request.client_secret,
        client_assertion_type: request.client_assertion_type,
        client_assertion: request.client_assertion,
    };
    let client = authenticate_client(
        &app_state,
        &headers,
        &issuer,
        &auth_req,
        oauth_app.id,
        "/oauth/introspect",
    )
    .await
    .map_err(map_token_auth_error)?;

    let token_hash = hash_value(token_value);
    let token =
        GetRuntimeIntrospectionDataQuery::new(oauth_app.deployment_id, client.id, token_hash)
            .execute(&app_state)
            .await
            .map_err(map_token_app_error)?;

    let Some(token) = token else {
        return Ok(Json(OAuthIntrospectResponse {
            active: false,
            scope: None,
            client_id: None,
            token_type: None,
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            nbf: None,
            sub: None,
            resource: None,
            granted_resource: None,
        }));
    };

    if !token.active {
        return Ok(Json(OAuthIntrospectResponse {
            active: false,
            scope: None,
            client_id: None,
            token_type: None,
            iss: None,
            aud: None,
            exp: None,
            iat: None,
            nbf: None,
            sub: None,
            resource: None,
            granted_resource: None,
        }));
    }
    if let Some(grant_id) = token.oauth_grant_id {
        enqueue_grant_last_used(
            app_state.clone(),
            oauth_app.deployment_id,
            client.id,
            grant_id,
        );
    }

    Ok(Json(OAuthIntrospectResponse {
        active: true,
        scope: Some(token.scopes.join(" ")),
        client_id: Some(token.client_id),
        token_type: Some("Bearer".to_string()),
        iss: Some(issuer),
        aud: token.granted_resource.clone(),
        exp: Some(token.expires_at.timestamp()),
        iat: Some(token.issued_at.timestamp()),
        nbf: Some(token.issued_at.timestamp()),
        sub: Some(token.app_slug),
        resource: token.resource,
        granted_resource: token.granted_resource,
    }))
}

pub async fn oauth_register_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<OAuthDynamicClientRegistrationRequest>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    if !oauth_app.allow_dynamic_client_registration {
        return Err((
            StatusCode::FORBIDDEN,
            "Dynamic client registration is disabled for this OAuth app",
        )
            .into());
    }

    let grant_types = if request.grant_types.is_empty() {
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ]
    } else {
        request.grant_types.clone()
    };
    let method = request
        .token_endpoint_auth_method
        .clone()
        .unwrap_or_else(|| "client_secret_basic".to_string());

    let created = CreateOAuthClientCommand {
        deployment_id: oauth_app.deployment_id,
        oauth_app_id: oauth_app.id,
        client_auth_method: method.clone(),
        grant_types: grant_types.clone(),
        redirect_uris: request.redirect_uris.clone(),
        client_name: request.client_name.clone(),
        client_uri: request.client_uri.clone(),
        logo_uri: request.logo_uri.clone(),
        tos_uri: request.tos_uri.clone(),
        policy_uri: request.policy_uri.clone(),
        contacts: request.contacts.clone(),
        software_id: request.software_id.clone(),
        software_version: request.software_version.clone(),
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?;

    let registration_access_token = generate_registration_access_token();
    let registration_access_token_hash = hash_value(&registration_access_token);
    SetOAuthClientRegistrationAccessToken {
        oauth_app_id: oauth_app.id,
        client_id: created.client.client_id.clone(),
        registration_access_token_hash: Some(registration_access_token_hash),
    }
    .execute(&app_state)
    .await?;

    let issued_at = created.client.created_at.timestamp();
    let created_client_id = created.client.client_id.clone();
    let created_contacts = created.client.contacts_vec();
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let client_secret_expires_at = if created.client_secret.is_some() {
        Some(0)
    } else {
        None
    };
    Ok(OAuthDynamicClientRegistrationResponse {
        client_id: created_client_id.clone(),
        client_name: created.client.client_name,
        client_uri: created.client.client_uri,
        logo_uri: created.client.logo_uri,
        tos_uri: created.client.tos_uri,
        policy_uri: created.client.policy_uri,
        contacts: created_contacts,
        software_id: created.client.software_id,
        software_version: created.client.software_version,
        client_secret: created.client_secret,
        client_id_issued_at: issued_at,
        client_secret_expires_at,
        token_endpoint_auth_method: method,
        grant_types,
        redirect_uris: request.redirect_uris,
        registration_client_uri: format!("{}/oauth/register/{}", issuer, created_client_id),
        registration_access_token: Some(registration_access_token),
    }
    .into())
}

pub async fn oauth_get_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, params.client_id.clone())
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    ensure_registration_access_token(&headers, client.registration_access_token_hash.as_deref())?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;

    Ok(OAuthDynamicClientRegistrationResponse {
        client_id: client.client_id.clone(),
        client_name: client.client_name,
        client_uri: client.client_uri,
        logo_uri: client.logo_uri,
        tos_uri: client.tos_uri,
        policy_uri: client.policy_uri,
        contacts: client.contacts,
        software_id: client.software_id,
        software_version: client.software_version,
        client_secret: None,
        client_id_issued_at: client.created_at.timestamp(),
        client_secret_expires_at: client_secret_expires_at_for_method(&client.client_auth_method),
        token_endpoint_auth_method: client.client_auth_method.clone(),
        grant_types: client.grant_types.clone(),
        redirect_uris: client.redirect_uris.clone(),
        registration_client_uri: format!("{}/oauth/register/{}", issuer, client.client_id),
        registration_access_token: None,
    }
    .into())
}

pub async fn oauth_update_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
    Json(request): Json<OAuthDynamicClientUpdateRequest>,
) -> ApiResult<OAuthDynamicClientRegistrationResponse> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let existing =
        GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, params.client_id.clone())
            .execute(&app_state)
            .await?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;
    ensure_registration_access_token(&headers, existing.registration_access_token_hash.as_deref())?;

    let updated = UpdateOAuthClientSettings {
        oauth_app_id: oauth_app.id,
        client_id: params.client_id.clone(),
        client_name: request.client_name.clone(),
        client_uri: request.client_uri.clone(),
        logo_uri: request.logo_uri.clone(),
        tos_uri: request.tos_uri.clone(),
        policy_uri: request.policy_uri.clone(),
        contacts: request.contacts.clone(),
        software_id: request.software_id.clone(),
        software_version: request.software_version.clone(),
        client_auth_method: request.token_endpoint_auth_method.clone(),
        grant_types: request.grant_types.clone(),
        redirect_uris: request.redirect_uris.clone(),
        token_endpoint_auth_signing_alg: request.token_endpoint_auth_signing_alg,
        jwks_uri: request.jwks_uri,
        jwks: request.jwks,
        public_key_pem: request.public_key_pem,
    }
    .execute(&app_state)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let updated_client_id = updated.client_id.clone();
    let updated_method = updated.client_auth_method.clone();
    let updated_grant_types = updated.grant_types_vec();
    let updated_redirect_uris = updated.redirect_uris_vec();
    let updated_contacts = updated.contacts_vec();
    Ok(OAuthDynamicClientRegistrationResponse {
        client_id: updated_client_id.clone(),
        client_name: updated.client_name,
        client_uri: updated.client_uri,
        logo_uri: updated.logo_uri,
        tos_uri: updated.tos_uri,
        policy_uri: updated.policy_uri,
        contacts: updated_contacts,
        software_id: updated.software_id,
        software_version: updated.software_version,
        client_secret: None,
        client_id_issued_at: updated.created_at.timestamp(),
        client_secret_expires_at: client_secret_expires_at_for_method(&updated_method),
        token_endpoint_auth_method: updated_method,
        grant_types: updated_grant_types,
        redirect_uris: updated_redirect_uris,
        registration_client_uri: format!("{}/oauth/register/{}", issuer, updated_client_id),
        registration_access_token: None,
    }
    .into())
}

pub async fn oauth_delete_registered_client(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Path(params): Path<OAuthRegisterPathParams>,
) -> ApiResult<()> {
    let oauth_app = resolve_oauth_app_from_host(&app_state, &headers).await?;
    let existing =
        GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, params.client_id.clone())
            .execute(&app_state)
            .await?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;
    ensure_registration_access_token(&headers, existing.registration_access_token_hash.as_deref())?;

    let _ = DeactivateOAuthClient {
        oauth_app_id: oauth_app.id,
        client_id: params.client_id,
    }
    .execute(&app_state)
    .await?;

    Ok((StatusCode::NO_CONTENT, ()).into())
}
