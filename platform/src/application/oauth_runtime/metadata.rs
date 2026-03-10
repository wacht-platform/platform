use super::*;

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

