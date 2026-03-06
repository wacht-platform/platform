use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::Redirect,
};
use commands::{
    ConsumeOAuthAuthorizationCode, CreateOAuthClientCommand, DeactivateOAuthClient,
    EnqueueOAuthGrantLastUsed, IssueOAuthAuthorizationCode, IssueOAuthTokenPair,
    RevokeOAuthAccessTokenByHash, RevokeOAuthRefreshTokenByHash, RevokeOAuthRefreshTokenById,
    RevokeOAuthRefreshTokenFamily, RevokeOAuthTokensByGrant, SetOAuthClientRegistrationAccessToken,
    SetOAuthRefreshTokenReplacement, UpdateOAuthClientSettings,
    api_key_app::EnsureUserApiAuthAppCommand,
};
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::oauth_runtime::{
    OAuthAuthorizeInitiatedResponse, OAuthAuthorizeRequest, OAuthConsentSubmitRequest,
    OAuthDynamicClientRegistrationRequest, OAuthDynamicClientRegistrationResponse,
    OAuthDynamicClientUpdateRequest, OAuthIntrospectRequest, OAuthIntrospectResponse,
    OAuthProtectedResourceMetadataResponse, OAuthRegisterPathParams, OAuthRevokeRequest,
    OAuthRevokeResponse, OAuthServerMetadataResponse, OAuthTokenRequest, OAuthTokenResponse,
};
use models::api_key::OAuthScopeDefinition;
use models::error::AppError;
use queries::{
    GetRuntimeAuthorizationCodeForExchangeQuery, GetRuntimeDeploymentHostsByIdQuery,
    GetRuntimeIntrospectionDataQuery, GetRuntimeOAuthClientByClientIdQuery,
    GetRuntimeRefreshTokenForExchangeQuery, RuntimeOAuthAppData, RuntimeOAuthClientData,
};
use redis::AsyncCommands;

use crate::{
    api::oauth_runtime::{
        helpers::{
            append_oauth_redirect_params, authenticate_client, client_secret_expires_at_for_method,
            derive_shared_secret, ensure_or_create_grant_coverage,
            ensure_registration_access_token, generate_prefixed_token,
            generate_registration_access_token, hash_value, is_valid_granted_resource_indicator,
            is_valid_resource_indicator, oauth_consent_backend_base_url,
            oauth_consent_handoff_redis_key, parse_scope_string, resolve_issuer_from_oauth_app,
            resolve_oauth_app_from_host, sign_oauth_consent_request_token,
            validate_grant_and_entitlement, verify_oauth_consent_request_token, verify_pkce,
        },
        token_handlers::{
            OAuthEndpointError, map_token_app_error, map_token_auth_error, map_token_pkce_error,
            oauth_token_error,
        },
        types::GrantValidationResult,
        types::{OAuthConsentHandoffPayload, OAuthConsentRequestTokenClaims},
    },
    application::response::ApiErrorResponse,
};

fn enqueue_grant_last_used(
    app_state: AppState,
    deployment_id: i64,
    oauth_client_id: i64,
    grant_id: i64,
) {
    tokio::spawn(async move {
        let _ = EnqueueOAuthGrantLastUsed {
            deployment_id,
            oauth_client_id,
            grant_id,
        }
        .execute_with(&app_state.redis_client)
        .await;
    });
}

pub async fn oauth_revoke(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthRevokeRequest,
) -> Result<Json<OAuthRevokeResponse>, OAuthEndpointError> {
    let OAuthRevokeRequest {
        token,
        token_type_hint,
        client_id,
        client_secret,
        client_assertion_type,
        client_assertion,
    } = request;

    let token_value = required_token_value(token.as_str())?;
    let (oauth_app, client, _) = authenticate_management_endpoint(
        &app_state,
        &headers,
        "/oauth/revoke",
        client_id,
        client_secret,
        client_assertion_type,
        client_assertion,
    )
    .await?;

    let hash = hash_value(token_value);
    let hint = token_type_hint.unwrap_or_default();
    if hint != "refresh_token" {
        RevokeOAuthAccessTokenByHash {
            deployment_id: oauth_app.deployment_id,
            oauth_client_id: client.id,
            token_hash: hash.clone(),
        }
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(map_token_app_error)?;
    }
    if hint != "access_token" {
        RevokeOAuthRefreshTokenByHash {
            deployment_id: oauth_app.deployment_id,
            oauth_client_id: client.id,
            token_hash: hash,
        }
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(map_token_app_error)?;
    }

    Ok(Json(OAuthRevokeResponse { revoked: true }))
}

pub async fn oauth_introspect(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthIntrospectRequest,
) -> Result<Json<OAuthIntrospectResponse>, OAuthEndpointError> {
    let OAuthIntrospectRequest {
        token,
        token_type_hint: _,
        client_id,
        client_secret,
        client_assertion_type,
        client_assertion,
    } = request;

    let token_value = required_token_value(token.as_str())?;
    let (oauth_app, client, issuer) = authenticate_management_endpoint(
        &app_state,
        &headers,
        "/oauth/introspect",
        client_id,
        client_secret,
        client_assertion_type,
        client_assertion,
    )
    .await?;

    let token_hash = hash_value(token_value);
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let token =
        GetRuntimeIntrospectionDataQuery::new(oauth_app.deployment_id, client.id, token_hash)
            .execute_with(reader)
            .await
            .map_err(map_token_app_error)?;

    let Some(token) = token else {
        return Ok(inactive_introspection_response());
    };

    if !token.active {
        return Ok(inactive_introspection_response());
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

pub async fn oauth_token(
    app_state: AppState,
    headers: HeaderMap,
    request: OAuthTokenRequest,
) -> Result<Json<OAuthTokenResponse>, OAuthEndpointError> {
    let context = resolve_token_context(&app_state, &headers, &request).await?;
    ensure_client_allows_grant_type(&context.client, &request.grant_type)?;

    match request.grant_type.as_str() {
        "authorization_code" => {
            handle_authorization_code_grant(&app_state, &request, &context).await
        }
        "refresh_token" => handle_refresh_token_grant(&app_state, &request, &context).await,
        _ => Err(unsupported_grant_type_error()),
    }
}

pub async fn oauth_server_metadata(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<OAuthServerMetadataResponse, ApiErrorResponse> {
    let (oauth_app, issuer) = resolve_oauth_app_and_issuer(app_state, headers).await?;
    let active_scopes = oauth_app.active_scopes();

    Ok(OAuthServerMetadataResponse {
        issuer: issuer.clone(),
        authorization_endpoint: format!("{}/oauth/authorize", issuer),
        token_endpoint: format!("{}/oauth/token", issuer),
        revocation_endpoint: format!("{}/oauth/revoke", issuer),
        introspection_endpoint: format!("{}/oauth/introspect", issuer),
        registration_endpoint: format!("{}/oauth/register", issuer),
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        token_endpoint_auth_methods_supported: vec![
            "client_secret_basic".to_string(),
            "client_secret_post".to_string(),
            "client_secret_jwt".to_string(),
            "private_key_jwt".to_string(),
            "none".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string()],
        scopes_supported: active_scopes,
    })
}

pub async fn oauth_protected_resource_metadata(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<OAuthProtectedResourceMetadataResponse, ApiErrorResponse> {
    let (oauth_app, issuer) = resolve_oauth_app_and_issuer(app_state, headers).await?;

    Ok(OAuthProtectedResourceMetadataResponse {
        resource: issuer.clone(),
        authorization_servers: vec![issuer],
        bearer_methods_supported: vec!["header".to_string()],
        scopes_supported: oauth_app.active_scopes(),
    })
}

pub async fn oauth_authorize_get(
    app_state: &AppState,
    headers: &HeaderMap,
    request: OAuthAuthorizeRequest,
) -> Result<Redirect, ApiErrorResponse> {
    let request_ctx = request.clone();
    match authorize_impl(app_state, headers, request).await {
        Ok(initiated) => Ok(Redirect::temporary(&initiated.consent_url)),
        Err(err) => {
            if let Some(redirect) =
                try_build_authorize_error_redirect(app_state, headers, &request_ctx, &err).await
            {
                return Ok(Redirect::temporary(&redirect));
            }
            Err(err)
        }
    }
}

pub async fn oauth_consent_submit(
    app_state: &AppState,
    headers: &HeaderMap,
    request: OAuthConsentSubmitRequest,
) -> Result<Redirect, ApiErrorResponse> {
    validate_consent_submit_secret(headers)?;
    let claims = verify_oauth_consent_request_token(&request.request_token)?;
    let (_, issuer) = resolve_oauth_app_and_issuer(app_state, headers).await?;
    let action = request.action.trim().to_ascii_lowercase();
    match action.as_str() {
        "approve" => {
            let approved_scopes = {
                match request.scope.as_deref() {
                    None => claims.scopes.clone(),
                    Some(raw_scope) => {
                        let requested = parse_scope_string(Some(raw_scope));
                        if requested.is_empty() {
                            Vec::new()
                        } else {
                            let approved: Vec<String> = requested
                                .into_iter()
                                .filter(|scope| claims.scopes.iter().any(|s| s == scope))
                                .collect();
                            if approved.is_empty() {
                                return Err((StatusCode::BAD_REQUEST, "invalid_scope").into());
                            }
                            approved
                        }
                    }
                }
            };
            let selected_resource = request
                .granted_resource
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| (StatusCode::BAD_REQUEST, "granted_resource is required"))?
                .to_string();
            if !is_valid_granted_resource_indicator(&selected_resource) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "granted_resource must be a canonical Wacht URN (e.g. urn:wacht:workspace:123)",
                )
                    .into());
            }
            let app_slug = EnsureUserApiAuthAppCommand::new(claims.deployment_id, request.user_id)
                .execute_with(app_state.db_router.writer())
                .await?;

            let oauth_grant_id = ensure_or_create_grant_coverage(
                app_state,
                claims.deployment_id,
                claims.oauth_client_id,
                app_slug.clone(),
                approved_scopes.clone(),
                selected_resource.clone(),
                request.user_id,
            )
            .await?;

            let issued = IssueOAuthAuthorizationCode {
                deployment_id: claims.deployment_id,
                oauth_client_id: claims.oauth_client_id,
                oauth_grant_id,
                app_slug,
                redirect_uri: claims.redirect_uri.clone(),
                code_challenge: claims.code_challenge,
                code_challenge_method: claims.code_challenge_method,
                scopes: approved_scopes,
                resource: claims.resource,
                granted_resource: Some(selected_resource),
            }
            .execute_with(
                app_state.db_router.writer(),
                app_state
                    .sf
                    .next_id()
                    .map_err(|e| AppError::Internal(e.to_string()))? as i64,
            )
            .await?;

            let redirect_uri = build_consent_redirect_uri(
                claims.redirect_uri,
                claims.state,
                &issuer,
                &[("code", issued.code)],
            );
            Ok(Redirect::to(&redirect_uri))
        }
        "deny" => {
            let redirect_uri = build_consent_redirect_uri(
                claims.redirect_uri,
                claims.state,
                &issuer,
                &[("error", "access_denied".to_string())],
            );
            Ok(Redirect::to(&redirect_uri))
        }
        _ => Err((StatusCode::BAD_REQUEST, "action must be approve or deny").into()),
    }
}

pub async fn oauth_register_client(
    app_state: &AppState,
    headers: &HeaderMap,
    request: OAuthDynamicClientRegistrationRequest,
) -> Result<OAuthDynamicClientRegistrationResponse, ApiErrorResponse> {
    let writer = app_state.db_router.writer();
    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await?;
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
    .execute_with(
        writer,
        &app_state.encryption_service,
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .await?;

    let registration_access_token = generate_registration_access_token();
    let registration_access_token_hash = hash_value(&registration_access_token);
    let created_client_id = created.client.client_id.clone();
    SetOAuthClientRegistrationAccessToken {
        oauth_app_id: oauth_app.id,
        client_id: created_client_id,
        registration_access_token_hash: Some(registration_access_token_hash),
    }
    .execute_with(writer)
    .await?;

    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    Ok(map_oauth_client_registration_response(
        created.client,
        &issuer,
        created.client_secret,
        Some(registration_access_token),
    ))
}

pub async fn oauth_get_registered_client(
    app_state: &AppState,
    headers: &HeaderMap,
    params: OAuthRegisterPathParams,
) -> Result<OAuthDynamicClientRegistrationResponse, ApiErrorResponse> {
    let (oauth_app, client) =
        resolve_registered_client_with_access(app_state, headers, &params.client_id).await?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    Ok(map_runtime_client_registration_response(client, &issuer))
}

pub async fn oauth_update_registered_client(
    app_state: &AppState,
    headers: &HeaderMap,
    params: OAuthRegisterPathParams,
    request: OAuthDynamicClientUpdateRequest,
) -> Result<OAuthDynamicClientRegistrationResponse, ApiErrorResponse> {
    let writer = app_state.db_router.writer();
    let (oauth_app, _) =
        resolve_registered_client_with_access(app_state, headers, &params.client_id).await?;

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
    .execute_with(writer)
    .await?
    .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    Ok(map_oauth_client_registration_response(
        updated, &issuer, None, None,
    ))
}

pub async fn oauth_delete_registered_client(
    app_state: &AppState,
    headers: &HeaderMap,
    params: OAuthRegisterPathParams,
) -> Result<(), ApiErrorResponse> {
    let writer = app_state.db_router.writer();
    let (oauth_app, _) =
        resolve_registered_client_with_access(app_state, headers, &params.client_id).await?;

    let _ = DeactivateOAuthClient {
        oauth_app_id: oauth_app.id,
        client_id: params.client_id,
    }
    .execute_with(writer)
    .await?;

    Ok(())
}

async fn authenticate_management_endpoint(
    app_state: &AppState,
    headers: &HeaderMap,
    endpoint_path: &str,
    client_id: Option<String>,
    client_secret: Option<String>,
    client_assertion_type: Option<String>,
    client_assertion: Option<String>,
) -> Result<(RuntimeOAuthAppData, RuntimeOAuthClientData, String), OAuthEndpointError> {
    let oauth_app = resolve_oauth_app_from_host(app_state, headers)
        .await
        .map_err(map_token_app_error)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app).map_err(map_token_app_error)?;
    let token_request = OAuthTokenRequest {
        grant_type: String::new(),
        code: None,
        redirect_uri: None,
        scope: None,
        code_verifier: None,
        refresh_token: None,
        client_id,
        client_secret,
        client_assertion_type,
        client_assertion,
    };
    let client = authenticate_client(
        app_state,
        headers,
        &issuer,
        &token_request,
        oauth_app.id,
        endpoint_path,
    )
    .await
    .map_err(map_token_auth_error)?;

    Ok((oauth_app, client, issuer))
}

async fn resolve_registered_client_with_access(
    app_state: &AppState,
    headers: &HeaderMap,
    client_id: &str,
) -> Result<(RuntimeOAuthAppData, RuntimeOAuthClientData), ApiErrorResponse> {
    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, client_id.to_string())
        .execute_with(reader)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    ensure_registration_access_token(headers, client.registration_access_token_hash.as_deref())?;
    Ok((oauth_app, client))
}

fn inactive_introspection_response() -> Json<OAuthIntrospectResponse> {
    Json(OAuthIntrospectResponse {
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
    })
}

fn map_oauth_client_registration_response(
    client: queries::oauth::OAuthClientData,
    issuer: &str,
    client_secret: Option<String>,
    registration_access_token: Option<String>,
) -> OAuthDynamicClientRegistrationResponse {
    let client_id = client.client_id.clone();
    let token_endpoint_auth_method = client.client_auth_method.clone();
    let contacts = client.contacts_vec();
    let grant_types = client.grant_types_vec();
    let redirect_uris = client.redirect_uris_vec();

    OAuthDynamicClientRegistrationResponse {
        client_id: client_id.clone(),
        client_name: client.client_name,
        client_uri: client.client_uri,
        logo_uri: client.logo_uri,
        tos_uri: client.tos_uri,
        policy_uri: client.policy_uri,
        contacts,
        software_id: client.software_id,
        software_version: client.software_version,
        client_secret,
        client_id_issued_at: client.created_at.timestamp(),
        client_secret_expires_at: client_secret_expires_at_for_method(&token_endpoint_auth_method),
        token_endpoint_auth_method,
        grant_types,
        redirect_uris,
        registration_client_uri: format!("{}/oauth/register/{}", issuer, client_id),
        registration_access_token,
    }
}

fn map_runtime_client_registration_response(
    client: RuntimeOAuthClientData,
    issuer: &str,
) -> OAuthDynamicClientRegistrationResponse {
    let client_id = client.client_id.clone();
    let token_endpoint_auth_method = client.client_auth_method.clone();

    OAuthDynamicClientRegistrationResponse {
        client_id: client_id.clone(),
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
        client_secret_expires_at: client_secret_expires_at_for_method(&token_endpoint_auth_method),
        token_endpoint_auth_method,
        grant_types: client.grant_types,
        redirect_uris: client.redirect_uris,
        registration_client_uri: format!("{}/oauth/register/{}", issuer, client_id),
        registration_access_token: None,
    }
}

fn required_token_value(token: &str) -> Result<&str, OAuthEndpointError> {
    let token_value = token.trim();
    if token_value.is_empty() {
        return Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            Some("token is required"),
        ));
    }
    Ok(token_value)
}

struct TokenEndpointContext {
    oauth_app: RuntimeOAuthAppData,
    client: RuntimeOAuthClientData,
}

async fn resolve_token_context(
    app_state: &AppState,
    headers: &HeaderMap,
    request: &OAuthTokenRequest,
) -> Result<TokenEndpointContext, OAuthEndpointError> {
    let oauth_app = resolve_oauth_app_from_host(app_state, headers)
        .await
        .map_err(map_token_app_error)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app).map_err(map_token_app_error)?;
    let client = authenticate_client(
        app_state,
        headers,
        &issuer,
        request,
        oauth_app.id,
        "/oauth/token",
    )
    .await
    .map_err(map_token_auth_error)?;

    Ok(TokenEndpointContext { oauth_app, client })
}

async fn handle_authorization_code_grant(
    app_state: &AppState,
    request: &OAuthTokenRequest,
    context: &TokenEndpointContext,
) -> Result<Json<OAuthTokenResponse>, OAuthEndpointError> {
    let code = required_form_field(request.code.as_deref(), "code")?;
    let redirect_uri = required_form_field(request.redirect_uri.as_deref(), "redirect_uri")?;
    let code_row = GetRuntimeAuthorizationCodeForExchangeQuery::new(
        context.oauth_app.deployment_id,
        context.client.id,
        hash_value(code),
    )
    .execute_with(app_state.db_router.reader(ReadConsistency::Strong))
    .await
    .map_err(map_token_app_error)?
    .ok_or_else(invalid_grant_error)?;

    if code_row.redirect_uri != redirect_uri {
        return Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "invalid_grant",
            Some("redirect_uri mismatch"),
        ));
    }
    if context.client.client_auth_method == "none" && code_row.pkce_code_challenge.is_none() {
        return Err(invalid_grant_error());
    }

    verify_pkce(
        code_row.pkce_code_challenge.as_deref(),
        code_row.pkce_code_challenge_method.as_deref(),
        request.code_verifier.as_deref(),
    )
    .map_err(map_token_pkce_error)?;

    let grant_result = validate_grant_and_entitlement(
        app_state,
        context.oauth_app.deployment_id,
        context.client.id,
        code_row.oauth_grant_id,
        code_row.app_slug.clone(),
        code_row.scopes.clone(),
        code_row.granted_resource.clone(),
        &context.oauth_app.scope_definitions,
    )
    .await
    .map_err(map_token_app_error)?;
    if grant_result != GrantValidationResult::Active {
        return Err(invalid_grant_error());
    }

    let consumed = ConsumeOAuthAuthorizationCode {
        code_id: code_row.id,
    }
    .execute_with(app_state.db_router.writer())
    .await
    .map_err(map_token_app_error)?;
    if !consumed {
        if let Some(oauth_grant_id) = code_row.oauth_grant_id {
            let _ = RevokeOAuthTokensByGrant {
                deployment_id: context.oauth_app.deployment_id,
                oauth_client_id: context.client.id,
                oauth_grant_id,
            }
            .execute_with(app_state.db_router.writer())
            .await;
        }
        return Err(invalid_grant_error());
    }

    let oauth_grant_id = code_row.oauth_grant_id.ok_or_else(invalid_grant_error)?;
    let scope = code_row.scopes.join(" ");
    let access_token_id = app_state
        .sf
        .next_id()
        .map_err(|e| map_token_app_error(AppError::Internal(e.to_string())))?
        as i64;
    let refresh_token_id = app_state
        .sf
        .next_id()
        .map_err(|e| map_token_app_error(AppError::Internal(e.to_string())))?
        as i64;
    let tokens = IssueOAuthTokenPair {
        deployment_id: context.oauth_app.deployment_id,
        oauth_client_id: context.client.id,
        oauth_grant_id,
        app_slug: code_row.app_slug,
        scopes: code_row.scopes,
        resource: code_row.resource,
        granted_resource: code_row.granted_resource,
    }
    .execute_with(
        app_state.db_router.writer(),
        access_token_id,
        refresh_token_id,
    )
    .await
    .map_err(map_token_app_error)?;

    enqueue_grant_last_used(
        app_state.clone(),
        context.oauth_app.deployment_id,
        context.client.id,
        oauth_grant_id,
    );

    Ok(Json(OAuthTokenResponse {
        access_token: tokens.access_token,
        token_type: "Bearer".to_string(),
        expires_in: tokens.access_expires_in,
        refresh_token: tokens.refresh_token,
        scope,
    }))
}

async fn handle_refresh_token_grant(
    app_state: &AppState,
    request: &OAuthTokenRequest,
    context: &TokenEndpointContext,
) -> Result<Json<OAuthTokenResponse>, OAuthEndpointError> {
    let refresh_token = required_form_field(request.refresh_token.as_deref(), "refresh_token")?;
    let refresh_row = GetRuntimeRefreshTokenForExchangeQuery::new(
        context.oauth_app.deployment_id,
        context.client.id,
        hash_value(refresh_token),
    )
    .execute_with(app_state.db_router.reader(ReadConsistency::Strong))
    .await
    .map_err(map_token_app_error)?
    .ok_or_else(invalid_grant_error)?;

    let now = chrono::Utc::now();
    let is_active_refresh = refresh_row.revoked_at.is_none() && refresh_row.expires_at > now;
    if !is_active_refresh {
        if refresh_row.replaced_by_token_id.is_some() {
            let revoked_count = RevokeOAuthRefreshTokenFamily {
                deployment_id: context.oauth_app.deployment_id,
                oauth_client_id: context.client.id,
                root_refresh_token_id: refresh_row.id,
            }
            .execute_with(app_state.db_router.writer())
            .await
            .map_err(map_token_app_error)?;
            tracing::warn!(
                event = "oauth.refresh_token_reuse_detected",
                deployment_id = context.oauth_app.deployment_id,
                oauth_client_id = context.client.id,
                refresh_token_id = refresh_row.id,
                revoked_refresh_tokens = revoked_count,
                "Refresh token replay detected; refresh token family revoked",
            );
        }
        return Err(invalid_grant_error());
    }

    let grant_result = validate_grant_and_entitlement(
        app_state,
        context.oauth_app.deployment_id,
        context.client.id,
        refresh_row.oauth_grant_id,
        refresh_row.app_slug.clone(),
        refresh_row.scopes.clone(),
        refresh_row.granted_resource.clone(),
        &context.oauth_app.scope_definitions,
    )
    .await
    .map_err(map_token_app_error)?;
    if grant_result != GrantValidationResult::Active {
        return Err(invalid_grant_error());
    }

    let requested_scopes = parse_scope_string(request.scope.as_deref());
    let effective_scopes = if requested_scopes.is_empty() {
        refresh_row.scopes.clone()
    } else {
        let granted_scopes: std::collections::HashSet<String> =
            refresh_row.scopes.iter().cloned().collect();
        if !requested_scopes
            .iter()
            .all(|scope| granted_scopes.contains(scope))
        {
            return Err(oauth_token_error(
                StatusCode::BAD_REQUEST,
                "invalid_scope",
                Some("requested scope is not a subset of original grant"),
            ));
        }
        requested_scopes
    };

    let revoked = RevokeOAuthRefreshTokenById {
        refresh_token_id: refresh_row.id,
    }
    .execute_with(app_state.db_router.writer())
    .await
    .map_err(map_token_app_error)?;
    if !revoked {
        return Err(invalid_grant_error());
    }

    let oauth_grant_id = refresh_row.oauth_grant_id.ok_or_else(invalid_grant_error)?;
    let access_token_id = app_state
        .sf
        .next_id()
        .map_err(|e| map_token_app_error(AppError::Internal(e.to_string())))?
        as i64;
    let refresh_token_id = app_state
        .sf
        .next_id()
        .map_err(|e| map_token_app_error(AppError::Internal(e.to_string())))?
        as i64;
    let tokens = IssueOAuthTokenPair {
        deployment_id: context.oauth_app.deployment_id,
        oauth_client_id: context.client.id,
        oauth_grant_id,
        app_slug: refresh_row.app_slug,
        scopes: effective_scopes.clone(),
        resource: refresh_row.resource.clone(),
        granted_resource: refresh_row.granted_resource.clone(),
    }
    .execute_with(
        app_state.db_router.writer(),
        access_token_id,
        refresh_token_id,
    )
    .await
    .map_err(map_token_app_error)?;

    SetOAuthRefreshTokenReplacement {
        old_refresh_token_id: refresh_row.id,
        new_refresh_token_id: tokens.refresh_token_id,
    }
    .execute_with(app_state.db_router.writer())
    .await
    .map_err(map_token_app_error)?;

    enqueue_grant_last_used(
        app_state.clone(),
        context.oauth_app.deployment_id,
        context.client.id,
        oauth_grant_id,
    );

    Ok(Json(OAuthTokenResponse {
        access_token: tokens.access_token,
        token_type: "Bearer".to_string(),
        expires_in: tokens.access_expires_in,
        refresh_token: tokens.refresh_token,
        scope: effective_scopes.join(" "),
    }))
}

fn required_form_field<'a>(
    value: Option<&'a str>,
    field_name: &'static str,
) -> Result<&'a str, OAuthEndpointError> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            oauth_token_error(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                Some(&format!("{field_name} is required")),
            )
        })
}

fn invalid_grant_error() -> OAuthEndpointError {
    oauth_token_error(StatusCode::BAD_REQUEST, "invalid_grant", None)
}

fn unsupported_grant_type_error() -> OAuthEndpointError {
    oauth_token_error(StatusCode::BAD_REQUEST, "unsupported_grant_type", None)
}

fn ensure_client_allows_grant_type(
    client: &RuntimeOAuthClientData,
    grant_type: &str,
) -> Result<(), OAuthEndpointError> {
    if client
        .grant_types
        .iter()
        .any(|client_grant_type| client_grant_type == grant_type)
    {
        Ok(())
    } else {
        Err(oauth_token_error(
            StatusCode::BAD_REQUEST,
            "unauthorized_client",
            Some("grant_type is not allowed for this client"),
        ))
    }
}

async fn authorize_impl(
    app_state: &AppState,
    headers: &HeaderMap,
    request: OAuthAuthorizeRequest,
) -> Result<OAuthAuthorizeInitiatedResponse, ApiErrorResponse> {
    let response_type =
        required_authorize_param(request.response_type.as_deref(), "missing response_type")?;
    if response_type != "code" {
        return Err((
            StatusCode::BAD_REQUEST,
            "Only response_type=code is supported",
        )
            .into());
    }
    let client_id =
        required_authorize_param(request.client_id.as_deref(), "missing client_id")?.to_string();
    let redirect_uri =
        required_authorize_param(request.redirect_uri.as_deref(), "missing redirect_uri")?
            .to_string();

    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await?;
    let client = get_runtime_oauth_client(app_state, oauth_app.id, client_id).await?;
    if !client.is_active {
        return Err((StatusCode::BAD_REQUEST, "OAuth client is inactive").into());
    }
    if !client.grant_types.iter().any(|g| g == "authorization_code") {
        return Err((
            StatusCode::BAD_REQUEST,
            "authorization_code is not allowed for this client",
        )
            .into());
    }
    if !client.redirect_uris.iter().any(|u| u == &redirect_uri) {
        return Err((
            StatusCode::BAD_REQUEST,
            "redirect_uri is not registered for this client",
        )
            .into());
    }
    validate_public_client_pkce(&client.client_auth_method, &request)?;

    let final_scopes = parse_scope_string(request.scope.as_deref());
    let active_scopes = oauth_app.active_scopes();
    let unsupported_scopes: Vec<String> = final_scopes
        .iter()
        .filter(|scope| !active_scopes.iter().any(|s| s == *scope))
        .cloned()
        .collect();
    if !unsupported_scopes.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "Unsupported scopes requested: {}",
                unsupported_scopes.join(", ")
            ),
        )
            .into());
    }
    let final_resource = request
        .resource
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string());
    if let Some(resource) = final_resource.as_deref() {
        if !is_valid_resource_indicator(resource) {
            return Err((StatusCode::BAD_REQUEST, "resource must be an absolute URI").into());
        }
    }
    let resource_options = final_resource
        .as_ref()
        .map(|r| vec![r.clone()])
        .unwrap_or_default();
    let scope_definitions = resolve_scope_definitions(&oauth_app, &final_scopes);

    let iat = chrono::Utc::now().timestamp();
    let exp = iat + 600;
    let claims = OAuthConsentRequestTokenClaims {
        exp,
        iat,
        jti: generate_prefixed_token("ocrq", 16),
        deployment_id: oauth_app.deployment_id,
        oauth_client_id: client.id,
        client_id: client.client_id.clone(),
        redirect_uri,
        scopes: final_scopes,
        resource: final_resource,
        state: request.state,
        code_challenge: request.code_challenge,
        code_challenge_method: request.code_challenge_method,
    };
    let request_token = sign_oauth_consent_request_token(&claims)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let deployment_hosts = GetRuntimeDeploymentHostsByIdQuery::new(oauth_app.deployment_id)
        .execute_with(reader)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Deployment not found for OAuth app"))?;

    let handoff_id = generate_prefixed_token("och", 18);
    let handoff_key = oauth_consent_handoff_redis_key(&handoff_id);
    let handoff_payload = OAuthConsentHandoffPayload {
        request_token,
        issuer: issuer.clone(),
        deployment_id: oauth_app.deployment_id,
        oauth_client_id: client.id,
        client_id: claims.client_id,
        redirect_uri: claims.redirect_uri,
        scopes: claims.scopes,
        scope_definitions,
        resource: claims.resource,
        resource_options,
        state: claims.state,
        expires_at: claims.exp,
        client_name: client.client_name,
    };

    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to connect redis: {e}"),
            )
        })?;
    redis_conn
        .set_ex::<_, _, ()>(
            &handoff_key,
            serde_json::to_string(&handoff_payload).map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to encode handoff",
                )
            })?,
            600,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to store oauth handoff: {e}"),
            )
        })?;

    let backend_base = oauth_consent_backend_base_url(&deployment_hosts.backend_host);
    let consent_url = format!(
        "{}/oauth/consent/init?handoff_id={}",
        backend_base, handoff_id
    );
    Ok(OAuthAuthorizeInitiatedResponse {
        consent_url,
        expires_in: 600,
    })
}

async fn try_build_authorize_error_redirect(
    app_state: &AppState,
    headers: &HeaderMap,
    request: &OAuthAuthorizeRequest,
    err: &ApiErrorResponse,
) -> Option<String> {
    let redirect_uri = request
        .redirect_uri
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?;
    if redirect_uri.is_empty() {
        return None;
    }

    let client_id = request
        .client_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())?
        .to_string();
    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await.ok()?;
    let client = get_runtime_oauth_client(app_state, oauth_app.id, client_id)
        .await
        .ok()?;
    let redirect_registered = client.redirect_uris.iter().any(|u| u == redirect_uri);
    if !redirect_registered {
        return None;
    }

    let description = err
        .errors
        .first()
        .map(|e| e.message.clone())
        .unwrap_or_else(|| "invalid_request".to_string());
    let error_code = match description.as_str() {
        message if message.starts_with("Unsupported scopes requested") => "invalid_scope",
        "authorization_code is not allowed for this client" | "OAuth client is inactive" => {
            "unauthorized_client"
        }
        _ if err.staus_code == StatusCode::INTERNAL_SERVER_ERROR => "server_error",
        _ => "invalid_request",
    };

    let issuer = resolve_issuer_from_oauth_app(&oauth_app).ok();
    Some(append_oauth_redirect_params(
        redirect_uri.to_string(),
        &[
            ("error", error_code.to_string()),
            ("error_description", description),
        ],
        request.state.clone(),
        issuer,
    ))
}

fn validate_consent_submit_secret(headers: &HeaderMap) -> Result<(), ApiErrorResponse> {
    let expected = derive_shared_secret("oauth-consent-submit-v1").map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "OAuth consent submit secret is not configured",
        )
    })?;
    let provided = headers
        .get("X-OAuth-Consent-Secret")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                "Missing OAuth consent submit secret",
            )
        })?;
    if provided != expected {
        return Err((
            StatusCode::UNAUTHORIZED,
            "Invalid OAuth consent submit secret",
        )
            .into());
    }
    Ok(())
}

fn build_consent_redirect_uri(
    redirect_uri: String,
    state: Option<String>,
    issuer: &str,
    params: &[(&str, String)],
) -> String {
    append_oauth_redirect_params(redirect_uri, params, state, Some(issuer.to_string()))
}

async fn resolve_oauth_app_and_issuer(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<(RuntimeOAuthAppData, String), ApiErrorResponse> {
    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    Ok((oauth_app, issuer))
}

fn validate_public_client_pkce(
    client_auth_method: &str,
    request: &OAuthAuthorizeRequest,
) -> Result<(), ApiErrorResponse> {
    if client_auth_method != "none" {
        return Ok(());
    }

    let code_challenge = request
        .code_challenge
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "code_challenge is required for public clients",
            )
        })?;
    if code_challenge.len() < 43 || code_challenge.len() > 128 {
        return Err((StatusCode::BAD_REQUEST, "invalid code_challenge").into());
    }

    let method = request
        .code_challenge_method
        .as_deref()
        .unwrap_or("S256")
        .trim();
    if method != "S256" {
        return Err((StatusCode::BAD_REQUEST, "unsupported code_challenge_method").into());
    }

    Ok(())
}

fn resolve_scope_definitions(
    oauth_app: &RuntimeOAuthAppData,
    scopes: &[String],
) -> Vec<OAuthScopeDefinition> {
    scopes
        .iter()
        .map(|scope| {
            oauth_app
                .scope_definitions
                .iter()
                .find(|definition| definition.scope == *scope)
                .cloned()
                .unwrap_or_else(|| OAuthScopeDefinition {
                    scope: scope.clone(),
                    display_name: scope.clone(),
                    description: String::new(),
                    archived: false,
                    category: String::new(),
                    organization_permission: None,
                    workspace_permission: None,
                })
        })
        .collect()
}

fn required_authorize_param<'a>(
    value: Option<&'a str>,
    message: &'static str,
) -> Result<&'a str, ApiErrorResponse> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| (StatusCode::BAD_REQUEST, message).into())
}

async fn get_runtime_oauth_client(
    app_state: &AppState,
    oauth_app_id: i64,
    client_id: String,
) -> Result<RuntimeOAuthClientData, ApiErrorResponse> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetRuntimeOAuthClientByClientIdQuery::new(oauth_app_id, client_id)
        .execute_with(reader)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found").into())
}
