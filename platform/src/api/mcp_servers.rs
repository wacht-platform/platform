use crate::api::pagination::paginate_results;
use crate::application::response::{ApiResult, PaginatedResponse};
use crate::middleware::RequireDeployment;
use axum::extract::{Json, Path, Query, State};
use common::utils::ssrf::validate_webhook_url;
use rmcp::{
    ServiceExt,
    model::{ClientCapabilities, ClientInfo, Implementation},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::timeout;

use common::state::AppState;

use commands::{
    AttachMcpServerToAgentCommand, Command, CreateMcpServerCommand, DeleteMcpServerCommand,
    DetachMcpServerFromAgentCommand, UpdateMcpServerCommand,
};
use models::{McpAuthConfig, McpServer, McpServerConfig};
use queries::{
    GetAgentMcpServersQuery, GetDeploymentWithSettingsQuery, GetMcpServerByIdQuery,
    GetMcpServersQuery, Query as QueryTrait,
};

const MCP_OAUTH_CALLBACK_URL: &str =
    "https://agentlink.wacht.services/service/mcp/consent/callback";

#[derive(Deserialize)]
pub struct McpServerParams {
    pub mcp_server_id: i64,
}

#[derive(Deserialize)]
pub struct AgentMcpServerParams {
    pub agent_id: i64,
    pub mcp_server_id: i64,
}

#[derive(Deserialize)]
pub struct AgentParams {
    pub agent_id: i64,
}

#[derive(Deserialize)]
pub struct GetMcpServersQueryParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct CreateMcpServerRequest {
    pub name: String,
    pub config: McpServerConfig,
}

#[derive(Debug, Deserialize)]
pub struct UpdateMcpServerRequest {
    pub name: Option<String>,
    pub config: Option<McpServerConfig>,
}

#[derive(Debug, Deserialize)]
pub struct DiscoverMcpServerAuthRequest {
    pub endpoint: String,
}

#[derive(Debug, Serialize)]
pub struct McpAuthDiscoveryResult {
    pub requires_auth: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_auth_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub register_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_metadata_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub token_endpoint_auth_methods_supported: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub authorization_servers: Vec<String>,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct McpServerCreateResponse {
    #[serde(flatten)]
    pub server: McpServer,
    pub discovery_result: McpAuthDiscoveryResult,
}

fn parse_quoted_auth_param(header: &str, key: &str) -> Option<String> {
    let needle = format!("{key}=\"");
    let start = header.find(&needle)? + needle.len();
    let rest = &header[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn recommended_auth_mode_from_token_auth_methods(methods: &[String]) -> String {
    if methods.iter().any(|m| m.eq_ignore_ascii_case("none")) {
        "oauth_authorization_code_public_pkce".to_string()
    } else {
        "oauth_authorization_code_confidential_pkce".to_string()
    }
}

async fn discover_auth_metadata(
    endpoint: &str,
) -> Result<
    (
        Option<String>,
        Vec<String>,
        bool,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Vec<String>,
        Vec<String>,
    ),
    common::error::AppError,
> {
    validate_webhook_url(endpoint).map_err(common::error::AppError::BadRequest)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| common::error::AppError::Internal(format!("HTTP client error: {}", e)))?;

    let response = client.get(endpoint).send().await.map_err(|e| {
        common::error::AppError::BadRequest(format!("Auth discovery failed: {}", e))
    })?;

    let mut has_bearer_challenge = false;
    for value in response
        .headers()
        .get_all(reqwest::header::WWW_AUTHENTICATE)
    {
        if let Ok(raw) = value.to_str() {
            if raw.to_lowercase().contains("bearer") {
                has_bearer_challenge = true;
            }
        }
    }

    if !matches!(
        response.status(),
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Ok((
            None,
            Vec::new(),
            false,
            None,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
        ));
    }

    let mut resource_metadata_url: Option<String> = None;
    for value in response
        .headers()
        .get_all(reqwest::header::WWW_AUTHENTICATE)
    {
        if let Ok(raw) = value.to_str() {
            if let Some(url) = parse_quoted_auth_param(raw, "resource_metadata") {
                resource_metadata_url = Some(url);
                break;
            }
        }
    }

    let mut authorization_servers = Vec::new();
    let mut resource: Option<String> = None;
    let mut prm_scopes: Vec<String> = Vec::new();
    if let Some(url) = &resource_metadata_url {
        validate_webhook_url(url).map_err(common::error::AppError::BadRequest)?;
        let prm_response = client.get(url).send().await.map_err(|e| {
            common::error::AppError::BadRequest(format!("Failed to fetch resource metadata: {}", e))
        })?;

        if prm_response.status().is_success() {
            let prm_json = prm_response
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({}));
            authorization_servers = prm_json
                .get("authorization_servers")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            resource = prm_json
                .get("resource")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            prm_scopes = prm_json
                .get("scopes_supported")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
        }
    }

    let mut auth_url: Option<String> = None;
    let mut token_url: Option<String> = None;
    let mut register_url: Option<String> = None;
    let mut scopes = prm_scopes;
    let mut token_auth_methods: Vec<String> = Vec::new();

    if let Some(auth_server) = authorization_servers.first() {
        let auth_server = auth_server.trim_end_matches('/');
        let oauth_metadata_url = format!("{}/.well-known/oauth-authorization-server", auth_server);
        let oauth_response = client.get(&oauth_metadata_url).send().await.ok();

        if let Some(response) = oauth_response {
            if response.status().is_success() {
                if let Ok(metadata_json) = response.json::<serde_json::Value>().await {
                    auth_url = metadata_json
                        .get("authorization_endpoint")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    token_url = metadata_json
                        .get("token_endpoint")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    register_url = metadata_json
                        .get("registration_endpoint")
                        .and_then(|v| v.as_str())
                        .map(str::to_string);
                    token_auth_methods = metadata_json
                        .get("token_endpoint_auth_methods_supported")
                        .and_then(|v| v.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if scopes.is_empty() {
                        scopes = metadata_json
                            .get("scopes_supported")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(str::to_string))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                    }
                }
            }
        }
    }

    Ok((
        resource_metadata_url,
        authorization_servers,
        has_bearer_challenge,
        auth_url,
        token_url,
        register_url,
        resource,
        scopes,
        token_auth_methods,
    ))
}

async fn fetch_access_token_for_validation(
    config: &McpServerConfig,
) -> Result<Option<String>, common::error::AppError> {
    match &config.auth {
        None => Ok(None),
        Some(McpAuthConfig::Token { auth_token }) => Ok(Some(auth_token.clone())),
        Some(McpAuthConfig::OAuthClientCredentials {
            client_id,
            client_secret,
            token_url,
            scopes,
        }) => {
            let token_url = token_url.as_ref().ok_or_else(|| {
                common::error::AppError::BadRequest(
                    "token_url is required for OAuth client credentials validation".to_string(),
                )
            })?;
            validate_webhook_url(token_url).map_err(common::error::AppError::BadRequest)?;

            let scope = if scopes.is_empty() {
                None
            } else {
                Some(scopes.join(" "))
            };

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .map_err(|e| {
                    common::error::AppError::Internal(format!("HTTP client error: {}", e))
                })?;

            let mut form: Vec<(&str, String)> =
                vec![("grant_type", "client_credentials".to_string())];
            if let Some(scope) = scope {
                form.push(("scope", scope));
            }

            let response = client
                .post(token_url)
                .basic_auth(client_id, Some(client_secret))
                .form(&form)
                .send()
                .await
                .map_err(|e| {
                    common::error::AppError::BadRequest(format!(
                        "Failed to request OAuth access token: {}",
                        e
                    ))
                })?;

            if !response.status().is_success() {
                return Err(common::error::AppError::BadRequest(format!(
                    "OAuth token request failed with status {}",
                    response.status()
                )));
            }

            let token_json: serde_json::Value = response.json().await.map_err(|e| {
                common::error::AppError::BadRequest(format!("Invalid OAuth token response: {}", e))
            })?;

            let access_token = token_json
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    common::error::AppError::BadRequest(
                        "OAuth token response missing access_token".to_string(),
                    )
                })?;

            Ok(Some(access_token.to_string()))
        }
        Some(McpAuthConfig::OAuthAuthorizationCodePublicPkce { .. }) => Ok(None),
        Some(McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce { .. }) => Ok(None),
    }
}

async fn register_oauth_client(
    register_url: &str,
    token_endpoint_auth_method: &str,
    client_name: &str,
) -> Result<(String, Option<String>), common::error::AppError> {
    validate_webhook_url(register_url).map_err(common::error::AppError::BadRequest)?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| common::error::AppError::Internal(format!("HTTP client error: {}", e)))?;

    let payload = serde_json::json!({
        "client_name": client_name,
        "redirect_uris": [MCP_OAUTH_CALLBACK_URL],
        "grant_types": ["authorization_code"],
        "response_types": ["code"],
        "token_endpoint_auth_method": token_endpoint_auth_method
    });

    let response = client
        .post(register_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| {
            common::error::AppError::BadRequest(format!("Failed to register OAuth client: {}", e))
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(common::error::AppError::BadRequest(format!(
            "OAuth client registration failed with status {}: {}",
            status, body
        )));
    }

    let response_json: serde_json::Value = response.json().await.map_err(|e| {
        common::error::AppError::BadRequest(format!("Invalid OAuth registration response: {}", e))
    })?;

    let client_id = response_json
        .get("client_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| {
            common::error::AppError::BadRequest(
                "OAuth registration response missing client_id".to_string(),
            )
        })?;

    let client_secret = response_json
        .get("client_secret")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string);

    Ok((client_id.to_string(), client_secret))
}

async fn hydrate_and_register_mcp_auth_config(
    app_state: &AppState,
    deployment_id: i64,
    config: &mut McpServerConfig,
) -> Result<(), common::error::AppError> {
    validate_webhook_url(config.endpoint.trim()).map_err(common::error::AppError::BadRequest)?;

    match &mut config.auth {
        Some(McpAuthConfig::OAuthAuthorizationCodePublicPkce {
            client_id,
            auth_url,
            token_url,
            register_url,
            scopes,
            resource,
        }) => {
            let has_client_id = client_id
                .as_ref()
                .map(|v| !v.trim().is_empty())
                .unwrap_or(false);
            if has_client_id {
                return Ok(());
            }

            let (
                _resource_metadata_url,
                _authorization_servers,
                _has_bearer_challenge,
                discovered_auth_url,
                discovered_token_url,
                discovered_register_url,
                discovered_resource,
                discovered_scopes,
                _token_auth_methods,
            ) = discover_auth_metadata(&config.endpoint).await?;

            if auth_url
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
            {
                *auth_url = discovered_auth_url;
            }
            if token_url
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
            {
                *token_url = discovered_token_url;
            }
            if register_url
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
            {
                *register_url = discovered_register_url;
            }
            if resource
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
            {
                *resource = discovered_resource;
            }
            if scopes.is_empty() && !discovered_scopes.is_empty() {
                *scopes = discovered_scopes;
            }

            let register_endpoint = register_url
                .as_ref()
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    common::error::AppError::BadRequest(
                        "register_url is required for oauth_authorization_code_public_pkce when client_id is not set"
                            .to_string(),
                    )
                })?;

            let client_name = mcp_oauth_client_name(app_state, deployment_id).await?;
            let (generated_client_id, _) =
                register_oauth_client(register_endpoint, "none", &client_name).await?;
            *client_id = Some(generated_client_id);
            Ok(())
        }
        Some(McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce {
            client_id,
            client_secret,
            auth_url,
            token_url,
            scopes,
            resource,
        }) => {
            let has_client_id = !client_id.trim().is_empty();
            let has_client_secret = !client_secret.trim().is_empty();
            if has_client_id && has_client_secret {
                return Ok(());
            }

            let (
                _resource_metadata_url,
                _authorization_servers,
                _has_bearer_challenge,
                discovered_auth_url,
                discovered_token_url,
                discovered_register_url,
                discovered_resource,
                discovered_scopes,
                discovered_token_auth_methods,
            ) = discover_auth_metadata(&config.endpoint).await?;

            if auth_url
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
            {
                *auth_url = discovered_auth_url;
            }
            if token_url
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
            {
                *token_url = discovered_token_url;
            }
            if resource
                .as_ref()
                .map(|v| v.trim().is_empty())
                .unwrap_or(true)
            {
                *resource = discovered_resource;
            }
            if scopes.is_empty() && !discovered_scopes.is_empty() {
                *scopes = discovered_scopes;
            }

            let register_endpoint = discovered_register_url
                .as_ref()
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    common::error::AppError::BadRequest(
                        "register_url is required to auto-register oauth_authorization_code_confidential_pkce"
                            .to_string(),
                    )
                })?;

            let mut methods_to_try: Vec<&str> = Vec::new();
            if discovered_token_auth_methods
                .iter()
                .any(|m| m.eq_ignore_ascii_case("client_secret_basic"))
            {
                methods_to_try.push("client_secret_basic");
            }
            if discovered_token_auth_methods
                .iter()
                .any(|m| m.eq_ignore_ascii_case("client_secret_post"))
            {
                methods_to_try.push("client_secret_post");
            }
            if discovered_token_auth_methods
                .iter()
                .any(|m| m.eq_ignore_ascii_case("none"))
            {
                methods_to_try.push("none");
            }
            if methods_to_try.is_empty() {
                methods_to_try.extend(["client_secret_basic", "client_secret_post", "none"]);
            }

            let mut last_error: Option<String> = None;
            let client_name = mcp_oauth_client_name(app_state, deployment_id).await?;
            for method in methods_to_try {
                match register_oauth_client(register_endpoint, method, &client_name).await {
                    Ok((generated_client_id, generated_client_secret)) => {
                        if generated_client_id.trim().is_empty() {
                            last_error = Some(format!(
                                "OAuth registration using {} did not return client_id",
                                method
                            ));
                            continue;
                        }
                        let Some(generated_client_secret) = generated_client_secret else {
                            last_error = Some(format!(
                                "OAuth registration using {} did not return client_secret",
                                method
                            ));
                            continue;
                        };
                        *client_id = generated_client_id;
                        *client_secret = generated_client_secret;
                        return Ok(());
                    }
                    Err(error) => {
                        last_error = Some(error.to_string());
                    }
                }
            }

            Err(common::error::AppError::BadRequest(format!(
                "Failed to auto-register confidential OAuth client: {}",
                last_error.unwrap_or_else(|| "unknown registration error".to_string())
            )))
        }
        _ => Ok(()),
    }
}

async fn mcp_oauth_client_name(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<String, common::error::AppError> {
    let app_name = GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute(app_state)
        .await?
        .ui_settings
        .and_then(|ui| {
            let trimmed = ui.app_name.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .unwrap_or_else(|| "Wacht".to_string());

    Ok(format!("{app_name} MCP Client"))
}

fn validate_mcp_server_config(config: &McpServerConfig) -> Result<(), common::error::AppError> {
    let endpoint = config.endpoint.trim();
    if endpoint.is_empty() {
        return Err(common::error::AppError::BadRequest(
            "MCP endpoint cannot be empty".to_string(),
        ));
    }
    validate_webhook_url(endpoint).map_err(common::error::AppError::BadRequest)?;
    let endpoint_url = reqwest::Url::parse(endpoint)
        .map_err(|e| common::error::AppError::BadRequest(format!("Invalid endpoint URL: {}", e)))?;
    if !matches!(endpoint_url.scheme(), "http" | "https") {
        return Err(common::error::AppError::BadRequest(
            "MCP endpoint must use http or https".to_string(),
        ));
    }

    if let Some(auth) = &config.auth {
        match auth {
            McpAuthConfig::Token { auth_token } => {
                if auth_token.trim().is_empty() {
                    return Err(common::error::AppError::BadRequest(
                        "auth_token cannot be empty".to_string(),
                    ));
                }
            }
            McpAuthConfig::OAuthClientCredentials {
                client_id,
                client_secret,
                token_url,
                scopes: _,
            } => {
                if client_id.trim().is_empty() || client_secret.trim().is_empty() {
                    return Err(common::error::AppError::BadRequest(
                        "client_id and client_secret are required".to_string(),
                    ));
                }
                if let Some(url) = token_url {
                    let parsed = reqwest::Url::parse(url).map_err(|e| {
                        common::error::AppError::BadRequest(format!("Invalid token_url: {}", e))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(common::error::AppError::BadRequest(
                            "token_url must use http or https".to_string(),
                        ));
                    }
                }
            }
            McpAuthConfig::OAuthAuthorizationCodePublicPkce {
                client_id: _,
                auth_url,
                token_url,
                register_url,
                scopes: _,
                resource: _,
            } => {
                if let Some(url) = auth_url
                    .as_ref()
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty())
                {
                    let parsed = reqwest::Url::parse(url).map_err(|e| {
                        common::error::AppError::BadRequest(format!("Invalid auth_url: {}", e))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(common::error::AppError::BadRequest(
                            "auth_url must use http or https".to_string(),
                        ));
                    }
                }
                if let Some(url) = token_url
                    .as_ref()
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty())
                {
                    let parsed = reqwest::Url::parse(url).map_err(|e| {
                        common::error::AppError::BadRequest(format!("Invalid token_url: {}", e))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(common::error::AppError::BadRequest(
                            "token_url must use http or https".to_string(),
                        ));
                    }
                }
                if let Some(url) = register_url {
                    let parsed = reqwest::Url::parse(url).map_err(|e| {
                        common::error::AppError::BadRequest(format!("Invalid register_url: {}", e))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(common::error::AppError::BadRequest(
                            "register_url must use http or https".to_string(),
                        ));
                    }
                }
            }
            McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce {
                client_id,
                client_secret,
                auth_url,
                token_url,
                scopes: _,
                resource: _,
            } => {
                if client_id.trim().is_empty() || client_secret.trim().is_empty() {
                    return Err(common::error::AppError::BadRequest(
                        "client_id and client_secret are required".to_string(),
                    ));
                }
                if auth_url
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true)
                {
                    return Err(common::error::AppError::BadRequest(
                        "auth_url is required for oauth_authorization_code_confidential_pkce"
                            .to_string(),
                    ));
                }
                if token_url
                    .as_ref()
                    .map(|v| v.trim().is_empty())
                    .unwrap_or(true)
                {
                    return Err(common::error::AppError::BadRequest(
                        "token_url is required for oauth_authorization_code_confidential_pkce"
                            .to_string(),
                    ));
                }
                if let Some(url) = auth_url
                    .as_ref()
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty())
                {
                    let parsed = reqwest::Url::parse(url).map_err(|e| {
                        common::error::AppError::BadRequest(format!("Invalid auth_url: {}", e))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(common::error::AppError::BadRequest(
                            "auth_url must use http or https".to_string(),
                        ));
                    }
                }
                if let Some(url) = token_url
                    .as_ref()
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty())
                {
                    let parsed = reqwest::Url::parse(url).map_err(|e| {
                        common::error::AppError::BadRequest(format!("Invalid token_url: {}", e))
                    })?;
                    if !matches!(parsed.scheme(), "http" | "https") {
                        return Err(common::error::AppError::BadRequest(
                            "token_url must use http or https".to_string(),
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

async fn validate_mcp_server_runtime(
    config: &McpServerConfig,
) -> Result<(), common::error::AppError> {
    let auth_header = fetch_access_token_for_validation(config).await?;
    let transport_config = if let Some(token) = auth_header {
        StreamableHttpClientTransportConfig::with_uri(config.endpoint.clone()).auth_header(token)
    } else {
        StreamableHttpClientTransportConfig::with_uri(config.endpoint.clone())
    };
    let transport = StreamableHttpClientTransport::from_config(transport_config);
    let client_info = ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "wacht-platform-validation".to_string(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            website_url: None,
            icons: None,
        },
    };

    let client = timeout(Duration::from_secs(10), client_info.serve(transport))
        .await
        .map_err(|_| {
            common::error::AppError::BadRequest(
                "MCP validation timed out while connecting".to_string(),
            )
        })?
        .map_err(|e| {
            common::error::AppError::BadRequest(format!("MCP validation failed to connect: {}", e))
        })?;

    let validation_result = timeout(
        Duration::from_secs(10),
        client.list_tools(Default::default()),
    )
    .await
    .map_err(|_| {
        common::error::AppError::BadRequest(
            "MCP validation timed out while listing tools".to_string(),
        )
    })?
    .map(|_| ())
    .map_err(|e| {
        common::error::AppError::BadRequest(format!(
            "MCP validation failed during list_tools: {}",
            e
        ))
    });

    if let Err(error) = client.cancel().await {
        tracing::warn!("Failed to close MCP validation client cleanly: {}", error);
    }

    validation_result
}

pub async fn discover_mcp_server_auth(
    Json(request): Json<DiscoverMcpServerAuthRequest>,
) -> ApiResult<McpAuthDiscoveryResult> {
    let endpoint = request.endpoint.trim().to_string();
    if endpoint.is_empty() {
        return Err(common::error::AppError::BadRequest("endpoint is required".to_string()).into());
    }
    validate_webhook_url(&endpoint).map_err(common::error::AppError::BadRequest)?;

    let (
        resource_metadata_url,
        authorization_servers,
        has_bearer_challenge,
        auth_url,
        token_url,
        register_url,
        resource,
        scopes,
        token_auth_methods,
    ) = discover_auth_metadata(&endpoint).await?;
    if has_bearer_challenge || resource_metadata_url.is_some() || !authorization_servers.is_empty()
    {
        let recommended_mode = recommended_auth_mode_from_token_auth_methods(&token_auth_methods);
        return Ok(McpAuthDiscoveryResult {
            requires_auth: true,
            recommended_auth_mode: Some(recommended_mode),
            token_url,
            auth_url,
            register_url,
            resource_metadata_url,
            resource,
            scopes,
            token_endpoint_auth_methods_supported: token_auth_methods,
            authorization_servers,
            message: "Authorization required (detected from WWW-Authenticate header). Configure credentials before saving.".to_string(),
        }
        .into());
    }

    let probe_config = McpServerConfig {
        endpoint: endpoint.clone(),
        auth: None,
        headers: None,
    };

    match validate_mcp_server_runtime(&probe_config).await {
        Ok(_) => Ok(McpAuthDiscoveryResult {
            requires_auth: false,
            recommended_auth_mode: None,
            token_url: None,
            auth_url: None,
            register_url: None,
            resource_metadata_url: None,
            resource: None,
            scopes: Vec::new(),
            token_endpoint_auth_methods_supported: Vec::new(),
            authorization_servers: Vec::new(),
            message: "No authorization required. This MCP server can be used directly.".to_string(),
        }
        .into()),
        Err(err) => {
            let err_text = err.to_string().to_lowercase();
            if err_text.contains("401")
                || err_text.contains("403")
                || err_text.contains("authorization")
                || err_text.contains("unauthorized")
                || err_text.contains("auth required")
                || err_text.contains("invalid_token")
            {
                Ok(McpAuthDiscoveryResult {
                    requires_auth: true,
                    recommended_auth_mode: Some("oauth_authorization_code_public_pkce".to_string()),
                    token_url: None,
                    auth_url: None,
                    register_url: None,
                    resource_metadata_url: None,
                    resource: None,
                    scopes: Vec::new(),
                    token_endpoint_auth_methods_supported: Vec::new(),
                    authorization_servers: Vec::new(),
                    message: "Authorization required. Configure credentials before saving."
                        .to_string(),
                }
                .into())
            } else {
                Ok(McpAuthDiscoveryResult {
                    requires_auth: false,
                    recommended_auth_mode: None,
                    token_url: None,
                    auth_url: None,
                    register_url: None,
                    resource_metadata_url: None,
                    resource: None,
                    scopes: Vec::new(),
                    token_endpoint_auth_methods_supported: Vec::new(),
                    authorization_servers: Vec::new(),
                    message: format!(
                        "Could not conclusively detect authorization requirement: {}",
                        err
                    ),
                }
                .into())
            }
        }
    }
}

pub async fn get_mcp_servers(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Query(query): Query<GetMcpServersQueryParams>,
) -> ApiResult<PaginatedResponse<McpServer>> {
    let limit = query.limit.unwrap_or(50);
    let offset = query.offset;
    let servers = GetMcpServersQuery::new(deployment_id)
        .with_limit(Some(limit as u32 + 1))
        .with_offset(offset.map(|o| o as u32))
        .execute(&app_state)
        .await?;

    Ok(paginate_results(servers, limit as i32, offset).into())
}

pub async fn create_mcp_server(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateMcpServerRequest>,
) -> ApiResult<McpServerCreateResponse> {
    let mut config = request.config;
    hydrate_and_register_mcp_auth_config(&app_state, deployment_id, &mut config).await?;
    validate_mcp_server_config(&config)?;

    let discovery_result = if config.auth.is_none() {
        let (
            resource_metadata_url,
            authorization_servers,
            has_bearer_challenge,
            _auth_url,
            _token_url,
            _register_url,
            _resource,
            _scopes,
            token_auth_methods,
        ) = discover_auth_metadata(&config.endpoint).await?;
        if has_bearer_challenge
            || resource_metadata_url.is_some()
            || !authorization_servers.is_empty()
        {
            let recommended_mode =
                recommended_auth_mode_from_token_auth_methods(&token_auth_methods);
            return Err(common::error::AppError::BadRequest(format!(
                "Authorization required for this MCP server (detected from WWW-Authenticate header). Configure auth and retry. Suggested mode: {}. Token metadata URL: {}",
                recommended_mode,
                resource_metadata_url.unwrap_or_else(|| "unknown".to_string())
            ))
            .into());
        }

        let probe_config = McpServerConfig {
            endpoint: config.endpoint.clone(),
            auth: None,
            headers: config.headers.clone(),
        };

        match validate_mcp_server_runtime(&probe_config).await {
            Ok(_) => McpAuthDiscoveryResult {
                requires_auth: false,
                recommended_auth_mode: None,
                token_url: None,
                auth_url: None,
                register_url: None,
                resource_metadata_url: None,
                resource: None,
                scopes: Vec::new(),
                token_endpoint_auth_methods_supported: Vec::new(),
                authorization_servers: Vec::new(),
                message: "No authorization required. This MCP server can be used directly."
                    .to_string(),
            },
            Err(err) => {
                let err_text = err.to_string().to_lowercase();
                if err_text.contains("401")
                    || err_text.contains("403")
                    || err_text.contains("authorization")
                    || err_text.contains("unauthorized")
                    || err_text.contains("auth required")
                    || err_text.contains("invalid_token")
                {
                    return Err(common::error::AppError::BadRequest(format!(
                        "Authorization required for this MCP server. Configure auth and retry. Suggested mode: oauth_authorization_code_public_pkce.",
                    ))
                    .into());
                }
                McpAuthDiscoveryResult {
                    requires_auth: false,
                    recommended_auth_mode: None,
                    token_url: None,
                    auth_url: None,
                    register_url: None,
                    resource_metadata_url: None,
                    resource: None,
                    scopes: Vec::new(),
                    token_endpoint_auth_methods_supported: Vec::new(),
                    authorization_servers: Vec::new(),
                    message: format!("Validation note: {}", err),
                }
            }
        }
    } else {
        McpAuthDiscoveryResult {
            requires_auth: true,
            recommended_auth_mode: None,
            token_url: None,
            auth_url: None,
            register_url: None,
            resource_metadata_url: None,
            resource: None,
            scopes: Vec::new(),
            token_endpoint_auth_methods_supported: Vec::new(),
            authorization_servers: Vec::new(),
            message: "Authorization configured and validated.".to_string(),
        }
    };

    let is_runtime_validated = !matches!(
        config.auth,
        Some(McpAuthConfig::OAuthAuthorizationCodePublicPkce { .. })
            | Some(McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce { .. })
    );
    if is_runtime_validated {
        validate_mcp_server_runtime(&config).await?;
    }

    let server = CreateMcpServerCommand::new(deployment_id, request.name, config)
        .execute(&app_state)
        .await?;

    Ok(McpServerCreateResponse {
        server,
        discovery_result,
    }
    .into())
}

pub async fn get_mcp_server_by_id(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<McpServerParams>,
) -> ApiResult<McpServer> {
    let server = GetMcpServerByIdQuery::new(deployment_id, params.mcp_server_id)
        .execute(&app_state)
        .await?;
    Ok(server.into())
}

pub async fn update_mcp_server(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<McpServerParams>,
    Json(request): Json<UpdateMcpServerRequest>,
) -> ApiResult<McpServer> {
    let mut command = UpdateMcpServerCommand::new(deployment_id, params.mcp_server_id);

    if let Some(name) = request.name {
        command = command.with_name(name);
    }
    if let Some(config) = request.config {
        let mut config = config;
        hydrate_and_register_mcp_auth_config(&app_state, deployment_id, &mut config).await?;
        validate_mcp_server_config(&config)?;
        let is_runtime_validated = !matches!(
            config.auth,
            Some(McpAuthConfig::OAuthAuthorizationCodePublicPkce { .. })
                | Some(McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce { .. })
        );
        if is_runtime_validated {
            validate_mcp_server_runtime(&config).await?;
        }
        command = command.with_config(config);
    }

    let server = command.execute(&app_state).await?;
    Ok(server.into())
}

pub async fn delete_mcp_server(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<McpServerParams>,
) -> ApiResult<()> {
    DeleteMcpServerCommand::new(deployment_id, params.mcp_server_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn get_agent_mcp_servers(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentParams>,
) -> ApiResult<PaginatedResponse<McpServer>> {
    let servers = GetAgentMcpServersQuery::new(deployment_id, params.agent_id)
        .execute(&app_state)
        .await?;
    Ok(PaginatedResponse::from(servers).into())
}

pub async fn attach_mcp_server_to_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentMcpServerParams>,
) -> ApiResult<()> {
    AttachMcpServerToAgentCommand::new(deployment_id, params.agent_id, params.mcp_server_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn detach_mcp_server_from_agent(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<AgentMcpServerParams>,
) -> ApiResult<()> {
    DetachMcpServerFromAgentCommand::new(deployment_id, params.agent_id, params.mcp_server_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}
