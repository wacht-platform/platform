use super::*;

fn enqueue_grant_last_used(
    app_state: AppState,
    deployment_id: i64,
    oauth_client_id: i64,
    grant_id: i64,
) {
    tokio::spawn(async move {
        let redis_deps = deps::from_app(&app_state).redis();
        let _ = EnqueueOAuthGrantLastUsed {
            deployment_id,
            oauth_client_id,
            grant_id,
        }
        .execute_with_deps(&redis_deps)
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
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(map_token_app_error)?;
    }
    if hint != "access_token" {
        RevokeOAuthRefreshTokenByHash {
            deployment_id: oauth_app.deployment_id,
            oauth_client_id: client.id,
            token_hash: hash,
        }
        .execute_with_db(app_state.db_router.writer())
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
            .execute_with_db(reader)
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
    .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
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
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(map_token_app_error)?;
    if !consumed {
        if let Some(oauth_grant_id) = code_row.oauth_grant_id {
            let _ = RevokeOAuthTokensByGrant {
                deployment_id: context.oauth_app.deployment_id,
                oauth_client_id: context.client.id,
                oauth_grant_id,
            }
            .execute_with_db(app_state.db_router.writer())
            .await;
        }
        return Err(invalid_grant_error());
    }

    let oauth_grant_id = code_row.oauth_grant_id.ok_or_else(invalid_grant_error)?;
    let scope = code_row.scopes.join(" ");

    let scopes_for_id_token = code_row.scopes.clone();
    let wants_openid = scopes_for_id_token.iter().any(|s| s == "openid");
    let oidc_user_id = code_row.user_id;
    let oidc_nonce = code_row.nonce.clone();
    let oidc_auth_time = code_row
        .auth_time
        .map(|t| t.timestamp())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

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
    let access_token_format = context.client.access_token_format.clone();
    let access_token_ttl_seconds = context.client.access_token_ttl_seconds;
    let scopes_for_issue = code_row.scopes.clone();
    let granted_resource_for_jwt = code_row.granted_resource.clone();
    let app_slug_for_issue = code_row.app_slug.clone();
    let mut tokens = IssueOAuthTokenPair {
        access_token_id: Some(access_token_id),
        refresh_token_id: Some(refresh_token_id),
        deployment_id: context.oauth_app.deployment_id,
        oauth_client_id: context.client.id,
        oauth_grant_id,
        app_slug: app_slug_for_issue,
        scopes: scopes_for_issue.clone(),
        resource: code_row.resource,
        granted_resource: code_row.granted_resource,
        session_id: code_row.session_id,
        access_token_format: access_token_format.clone(),
        access_token_ttl_seconds,
    }
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(map_token_app_error)?;

    if access_token_format == "jwt" {
        let issuer =
            crate::api::oauth_runtime::helpers::resolve_issuer_from_oauth_app(&context.oauth_app)
                .map_err(map_token_app_error)?;
        tokens.access_token = super::oidc::build_access_jwt(
            &app_state,
            context.oauth_app.id,
            super::oidc::AccessJwtBuildContext {
                issuer,
                client_id: context.client.client_id.clone(),
                subject: oidc_user_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| context.client.client_id.clone()),
                session_id: code_row.session_id,
                scopes: scopes_for_issue,
                audience: granted_resource_for_jwt,
                ttl_seconds: access_token_ttl_seconds,
                access_token_id,
            },
        )
        .await
        .map_err(|e| {
            crate::api::oauth_runtime::token_handlers::oauth_token_error(
                e.status_code,
                "server_error",
                e.errors.first().map(|x| x.message.as_str()),
            )
        })?;
    }

    enqueue_grant_last_used(
        app_state.clone(),
        context.oauth_app.deployment_id,
        context.client.id,
        oauth_grant_id,
    );

    // openid → id_token is mandatory per OIDC. On signing failure revoke the
    // pair we just persisted so the store doesn't accumulate orphans (auth
    // code stays consumed per RFC 6749 §10.5 — client must redo /authorize).
    let id_token = if wants_openid {
        if let Some(user_id) = oidc_user_id {
            let issuer = crate::api::oauth_runtime::helpers::resolve_issuer_from_oauth_app(
                &context.oauth_app,
            )
            .map_err(map_token_app_error)?;
            let ctx = super::oidc::IdTokenBuildContext {
                issuer,
                client_id: context.client.client_id.clone(),
                deployment_id: context.oauth_app.deployment_id,
                user_id,
                session_id: code_row.session_id,
                auth_time: oidc_auth_time,
                nonce: oidc_nonce,
                access_token: tokens.access_token.clone(),
                scopes: scopes_for_id_token,
            };
            match super::oidc::build_id_token(&app_state, context.oauth_app.id, ctx).await {
                Ok(t) => Some(t),
                Err(err) => {
                    tracing::error!(
                        oauth_app_id = context.oauth_app.id,
                        "id_token issuance failed; rolling back issued token pair"
                    );
                    let access_hash =
                        crate::api::oauth_runtime::helpers::hash_value(&tokens.access_token);
                    let refresh_hash =
                        crate::api::oauth_runtime::helpers::hash_value(&tokens.refresh_token);
                    let writer = app_state.db_router.writer();
                    let _ = commands::oauth_runtime::RevokeOAuthAccessTokenByHash {
                        deployment_id: context.oauth_app.deployment_id,
                        oauth_client_id: context.client.id,
                        token_hash: access_hash,
                    }
                    .execute_with_db(writer)
                    .await;
                    let _ = commands::oauth_runtime::RevokeOAuthRefreshTokenByHash {
                        deployment_id: context.oauth_app.deployment_id,
                        oauth_client_id: context.client.id,
                        token_hash: refresh_hash,
                    }
                    .execute_with_db(writer)
                    .await;
                    return Err(
                        crate::api::oauth_runtime::token_handlers::oauth_token_error(
                            err.status_code,
                            "server_error",
                            err.errors.first().map(|e| e.message.as_str()),
                        ),
                    );
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    Ok(Json(OAuthTokenResponse {
        access_token: tokens.access_token,
        token_type: "Bearer".to_string(),
        expires_in: tokens.access_expires_in,
        refresh_token: tokens.refresh_token,
        scope,
        id_token,
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
    .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
    .await
    .map_err(map_token_app_error)?
    .ok_or_else(invalid_grant_error)?;

    let now = chrono::Utc::now();
    // RFC 6749 §10.4 replay detection: if the token has a successor, it has
    // already been rotated out — using it now is a captured-copy replay.
    // Must run before liveness checks (the row is by definition revoked once
    // rotation has happened).
    if refresh_row.replaced_by_token_id.is_some() {
        let revoked_count = RevokeOAuthRefreshTokenFamily {
            deployment_id: context.oauth_app.deployment_id,
            oauth_client_id: context.client.id,
            root_refresh_token_id: refresh_row.id,
        }
        .execute_with_db(app_state.db_router.writer())
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
        return Err(invalid_grant_error());
    }
    if refresh_row.revoked_at.is_some() || refresh_row.expires_at <= now {
        return Err(invalid_grant_error());
    }
    if refresh_row.session_deleted_at.is_some() {
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
    .execute_with_db(app_state.db_router.writer())
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
    let access_token_format = context.client.access_token_format.clone();
    let access_token_ttl_seconds = context.client.access_token_ttl_seconds;
    let mut tokens = IssueOAuthTokenPair {
        access_token_id: Some(access_token_id),
        refresh_token_id: Some(refresh_token_id),
        deployment_id: context.oauth_app.deployment_id,
        oauth_client_id: context.client.id,
        oauth_grant_id,
        app_slug: refresh_row.app_slug.clone(),
        scopes: effective_scopes.clone(),
        resource: refresh_row.resource.clone(),
        granted_resource: refresh_row.granted_resource.clone(),
        // OIDC: carry session linkage forward through refresh chains so the
        // logout cascade still reaches tokens minted from a refresh grant.
        session_id: refresh_row.session_id,
        access_token_format: access_token_format.clone(),
        access_token_ttl_seconds,
    }
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(map_token_app_error)?;

    if access_token_format == "jwt" {
        let issuer =
            crate::api::oauth_runtime::helpers::resolve_issuer_from_oauth_app(&context.oauth_app)
                .map_err(map_token_app_error)?;
        tokens.access_token = super::oidc::build_access_jwt(
            &app_state,
            context.oauth_app.id,
            super::oidc::AccessJwtBuildContext {
                issuer,
                client_id: context.client.client_id.clone(),
                subject: refresh_row
                    .user_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| context.client.client_id.clone()),
                session_id: refresh_row.session_id,
                scopes: effective_scopes.clone(),
                audience: refresh_row.granted_resource.clone(),
                ttl_seconds: access_token_ttl_seconds,
                access_token_id,
            },
        )
        .await
        .map_err(|e| {
            crate::api::oauth_runtime::token_handlers::oauth_token_error(
                e.status_code,
                "server_error",
                e.errors.first().map(|x| x.message.as_str()),
            )
        })?;
    }

    SetOAuthRefreshTokenReplacement {
        old_refresh_token_id: refresh_row.id,
        new_refresh_token_id: tokens.refresh_token_id,
    }
    .execute_with_db(app_state.db_router.writer())
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
        // OIDC id_token: refresh-grant path will populate this once we wire
        // the id_token issuance helper. For now (and for non-OIDC clients)
        // it stays absent.
        id_token: None,
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
