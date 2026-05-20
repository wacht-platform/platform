use super::*;

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
                .execute_with_db(app_state.db_router.writer())
                .await?;

            // OIDC `prompt=none`: don't create a new grant if one doesn't
            // already cover the request — that would silently auto-consent on
            // a no-UI request, which violates the spec. Lookup-only, redirect
            // with `error=consent_required` on miss so the RP can prompt
            // explicitly via a follow-up /authorize without prompt=none.
            let prompt_none = claims
                .prompt
                .as_deref()
                .map(|p| p.split_whitespace().any(|t| t == "none"))
                .unwrap_or(false);

            let oauth_grant_id = if prompt_none {
                let coverage = find_existing_grant_coverage(
                    app_state,
                    claims.deployment_id,
                    claims.oauth_client_id,
                    app_slug.clone(),
                    approved_scopes.clone(),
                    selected_resource.clone(),
                )
                .await?;
                match coverage {
                    Some(id) => id,
                    None => {
                        let redirect_uri = build_consent_redirect_uri(
                            claims.redirect_uri,
                            claims.state,
                            &issuer,
                            &[
                                ("error", "consent_required".to_string()),
                                (
                                    "error_description",
                                    "No existing consent covers the requested scopes".to_string(),
                                ),
                            ],
                        );
                        return Ok(Redirect::to(&redirect_uri));
                    }
                }
            } else {
                ensure_or_create_grant_coverage(
                    app_state,
                    claims.deployment_id,
                    claims.oauth_client_id,
                    app_slug.clone(),
                    approved_scopes.clone(),
                    selected_resource.clone(),
                    request.user_id,
                )
                .await?
            };

            // Session resolution order: consent-token claim → frontend-api
            // payload → most-recently-active session lookup (legacy fallback).
            let session_id = if claims.session_id.is_some() {
                claims.session_id
            } else if request.session_id.is_some() {
                request.session_id
            } else {
                queries::GetActiveSessionForUserQuery::new(claims.deployment_id, request.user_id)
                    .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
                    .await?
            };

            // OIDC `auth_time` per spec is the End-User authentication moment
            // (signins.created_at), not the /authorize timestamp.
            let auth_time = if let Some(sid) = session_id {
                queries::GetSigninAuthTimeQuery::new(sid, request.user_id)
                    .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
                    .await?
                    .or(claims.auth_time)
            } else {
                claims.auth_time
            };

            // OIDC `max_age` enforcement: refuse to issue a code when the
            // user's last authentication is older than the RP asked for.
            // The frontend would ideally re-auth the user first, but until
            // that's wired up we bail here with a redirect-style error so
            // the RP can decide to send the user back through /authorize.
            if let Some(max_age) = claims.max_age
                && let Some(authenticated_at) = auth_time
            {
                let elapsed = chrono::Utc::now()
                    .signed_duration_since(authenticated_at)
                    .num_seconds();
                if elapsed > max_age {
                    let redirect_uri = build_consent_redirect_uri(
                        claims.redirect_uri,
                        claims.state,
                        &issuer,
                        &[
                            ("error", "login_required".to_string()),
                            (
                                "error_description",
                                format!(
                                    "End-User authentication is older than max_age={max_age}s (elapsed {elapsed}s)"
                                ),
                            ),
                        ],
                    );
                    return Ok(Redirect::to(&redirect_uri));
                }
            }

            let issued = IssueOAuthAuthorizationCode {
                code_id: Some(
                    app_state
                        .sf
                        .next_id()
                        .map_err(|e| AppError::Internal(e.to_string()))? as i64,
                ),
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
                nonce: claims.nonce.clone(),
                auth_time,
                session_id,
            }
            .execute_with_db(app_state.db_router.writer())
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
    validate_oidc_authorize_params(&request)?;

    let final_scopes = parse_scope_string(request.scope.as_deref());
    let active_scopes = oauth_app.active_scopes();
    let unsupported_scopes: Vec<String> = final_scopes
        .iter()
        .filter(|scope| {
            // OIDC standard scopes are spec-defined and claims-driven, not
            // permission-driven, so they're implicitly supported on every
            // OAuth app regardless of the operator's scope catalog. This
            // mirrors what /.well-known/openid-configuration already
            // advertises and avoids a discovery-vs-authorize contradiction.
            if is_oidc_standard_scope(scope) {
                return false;
            }
            !active_scopes.iter().any(|s| s == *scope)
        })
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
        nonce: request.nonce,
        // /authorize-time placeholder; consent_submit overrides with the
        // signin's created_at (the spec-correct End-User auth time).
        auth_time: Some(chrono::Utc::now()),
        // Set at consent time from the live Wacht cookie.
        session_id: None,
        prompt: request
            .prompt
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned),
        max_age: request.max_age,
    };
    let request_token = sign_oauth_consent_request_token(&claims)?;
    let issuer = resolve_issuer_from_oauth_app(&oauth_app)?;
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let deployment_hosts = GetRuntimeDeploymentHostsByIdQuery::new(oauth_app.deployment_id)
        .execute_with_db(reader)
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
        prompt: claims.prompt.clone(),
        max_age: claims.max_age,
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
        _ if err.status_code == StatusCode::INTERNAL_SERVER_ERROR => "server_error",
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

/// Validate the OIDC `/authorize` parameters. Spec values that we honor (or
/// will honor downstream in the consent flow) are accepted; unknowns are
/// rejected so RPs fail loudly. Errors are mapped to OIDC error codes by
/// `oidc_authorize_error_code` and surfaced as redirects to `redirect_uri`.
fn validate_oidc_authorize_params(request: &OAuthAuthorizeRequest) -> Result<(), ApiErrorResponse> {
    if let Some(mode) = request
        .response_mode
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        && mode != "query"
    {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Unsupported response_mode `{mode}`; only `query` is advertised"),
        )
            .into());
    }

    if let Some(prompt_value) = request.prompt.as_deref() {
        for prompt in prompt_value.split_whitespace() {
            match prompt {
                // Always-show-UI is our default; these are no-ops.
                "login" | "consent" => {}
                // The consent flow can branch on these (skip UI / show picker)
                // once frontend-api wires the prompt through. Until then they
                // pass through and behave like the default.
                "none" | "select_account" => {}
                other => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        format!("Unknown prompt value `{other}`"),
                    )
                        .into());
                }
            }
        }
    }

    if let Some(max_age) = request.max_age
        && max_age < 0
    {
        return Err((StatusCode::BAD_REQUEST, "max_age must be non-negative").into());
    }

    Ok(())
}

/// The four OIDC-standard scopes that every OAuth app supports implicitly.
/// `openid` triggers id_token issuance; `profile`/`email` gate claims inside
/// `build_id_token` and `/userinfo`; `offline_access` signals refresh-token
/// intent. None of them grant permissions, so they don't belong in an app's
/// curated scope catalog.
fn is_oidc_standard_scope(scope: &str) -> bool {
    matches!(scope, "openid" | "profile" | "email" | "offline_access")
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
        .execute_with_db(reader)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "OAuth client not found").into())
}
