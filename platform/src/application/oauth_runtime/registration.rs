use super::*;

pub async fn oauth_register_client(
    app_state: &AppState,
    headers: &HeaderMap,
    request: OAuthDynamicClientRegistrationRequest,
) -> Result<OAuthDynamicClientRegistrationResponse, ApiErrorResponse> {
    let create_deps = deps::from_app(app_state).db().enc();
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
        client_record_id: Some(
            app_state
                .sf
                .next_id()
                .map_err(|e| AppError::Internal(e.to_string()))? as i64,
        ),
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
    .execute_with_deps(&create_deps)
    .await?;

    let registration_access_token = generate_registration_access_token();
    let registration_access_token_hash = hash_value(&registration_access_token);
    let created_client_id = created.client.client_id.clone();
    SetOAuthClientRegistrationAccessToken {
        oauth_app_id: oauth_app.id,
        client_id: created_client_id,
        registration_access_token_hash: Some(registration_access_token_hash),
    }
    .execute_with_db(app_state.db_router.writer())
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
    .execute_with_db(writer)
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
    .execute_with_db(writer)
    .await?;

    Ok(())
}

async fn resolve_registered_client_with_access(
    app_state: &AppState,
    headers: &HeaderMap,
    client_id: &str,
) -> Result<(RuntimeOAuthAppData, RuntimeOAuthClientData), ApiErrorResponse> {
    let oauth_app = resolve_oauth_app_from_host(app_state, headers).await?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let client = GetRuntimeOAuthClientByClientIdQuery::new(oauth_app.id, client_id.to_string())
        .execute_with_db(reader)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found"))?;

    ensure_registration_access_token(headers, client.registration_access_token_hash.as_deref())?;
    Ok((oauth_app, client))
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
