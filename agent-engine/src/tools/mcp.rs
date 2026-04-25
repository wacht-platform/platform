use chrono::Utc;
use common::error::AppError;
use models::{
    AiTool, AiToolConfiguration, AiToolType, McpAuthConfig, McpConnectionMetadata,
    McpServerConfig, McpToolConfiguration,
};
use queries::ActorMcpConnection;
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation},
    transport::{StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::time::{Duration, timeout};

use super::ToolExecutor;
use crate::filesystem::AgentFilesystem;

fn mcp_tool_synthetic_id(mcp_server_id: i64, tool_name: &str) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(mcp_server_id.to_le_bytes());
    hasher.update(tool_name.as_bytes());
    let bytes: [u8; 8] = hasher.finalize()[..8].try_into().unwrap();
    -(i64::from_le_bytes(bytes).abs().max(1))
}

// Stable tool name uses the server's DB id (immutable) rather than the mutable server name.
fn mcp_tool_agent_name(server_id: i64, tool_name: &str) -> String {
    format!("mcp_{}__{}", server_id, tool_name)
}

async fn fetch_client_credentials_token(
    client_id: &str,
    client_secret: &str,
    token_url: &str,
    scopes: &[String],
) -> Option<String> {
    let http = reqwest::Client::new();
    let mut params = vec![("grant_type", "client_credentials")];
    let scope_str;
    if !scopes.is_empty() {
        scope_str = scopes.join(" ");
        params.push(("scope", scope_str.as_str()));
    }
    let resp = http
        .post(token_url)
        .basic_auth(client_id, Some(client_secret))
        .form(&params)
        .send()
        .await
        .ok()?;
    let json: Value = resp.json().await.ok()?;
    json.get("access_token")?.as_str().map(|s| s.to_string())
}

struct RefreshedTokens {
    access_token: String,
    refresh_token: Option<String>,
    token_type: Option<String>,
    scope: Option<String>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

async fn try_refresh_oauth_token(
    refresh_token: &str,
    token_url: &str,
    auth_config: Option<&McpAuthConfig>,
    stored_client_id: Option<&str>,
) -> Option<RefreshedTokens> {
    let http = reqwest::Client::new();
    let mut form: Vec<(&str, String)> = vec![
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token.to_string()),
    ];

    let mut req = http.post(token_url);
    match auth_config {
        Some(McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce {
            client_id,
            client_secret,
            ..
        }) => {
            req = req.basic_auth(client_id, Some(client_secret));
        }
        Some(McpAuthConfig::OAuthClientCredentials {
            client_id,
            client_secret,
            ..
        }) => {
            req = req.basic_auth(client_id, Some(client_secret));
        }
        Some(McpAuthConfig::OAuthAuthorizationCodePublicPkce { client_id, .. }) => {
            if let Some(cid) = client_id.as_deref().or(stored_client_id) {
                form.push(("client_id", cid.to_string()));
            }
        }
        _ => {
            if let Some(cid) = stored_client_id {
                form.push(("client_id", cid.to_string()));
            }
        }
    }

    let resp = req.form(&form).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let access_token = json.get("access_token")?.as_str()?.to_string();
    Some(RefreshedTokens {
        access_token,
        refresh_token: json
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        token_type: json
            .get("token_type")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        scope: json
            .get("scope")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        expires_at: json
            .get("expires_in")
            .and_then(|v| v.as_i64())
            .map(|secs| Utc::now() + chrono::Duration::seconds(secs)),
    })
}

pub fn connection_needs_refresh(conn: &ActorMcpConnection) -> bool {
    let Some(meta) = conn.connection_metadata.as_ref() else {
        return false;
    };
    let is_expired = meta
        .expires_at
        .map(|exp| exp <= Utc::now())
        .unwrap_or(false);
    is_expired && meta.refresh_token.is_some() && meta.token_url.is_some()
}

pub async fn refresh_connection_metadata(
    conn: &ActorMcpConnection,
) -> Option<McpConnectionMetadata> {
    let meta = conn.connection_metadata.as_ref()?;
    let refresh_token = meta.refresh_token.as_deref()?;
    let token_url = meta.token_url.as_deref()?;

    let new_tokens = try_refresh_oauth_token(
        refresh_token,
        token_url,
        conn.server.config.auth.as_ref(),
        meta.oauth_client_id.as_deref(),
    )
    .await?;

    let mut new_meta = meta.clone();
    new_meta.access_token = new_tokens.access_token;
    if let Some(rt) = new_tokens.refresh_token {
        new_meta.refresh_token = Some(rt);
    }
    if let Some(tt) = new_tokens.token_type {
        new_meta.token_type = Some(tt);
    }
    if let Some(scope) = new_tokens.scope {
        new_meta.scope = Some(scope);
    }
    new_meta.expires_at = new_tokens.expires_at;
    Some(new_meta)
}

async fn resolve_bearer_token(
    server_config: &McpServerConfig,
    connection_metadata: Option<&McpConnectionMetadata>,
) -> Option<String> {
    if let Some(meta) = connection_metadata {
        return Some(meta.access_token.clone());
    }
    match server_config.auth.as_ref()? {
        McpAuthConfig::Token { auth_token } => Some(auth_token.clone()),
        McpAuthConfig::OAuthClientCredentials {
            client_id,
            client_secret,
            token_url,
            scopes,
        } => {
            let url = token_url.as_deref()?;
            fetch_client_credentials_token(client_id, client_secret, url, scopes).await
        }
        _ => None,
    }
}

async fn build_transport(
    server_config: &McpServerConfig,
    connection_metadata: Option<&McpConnectionMetadata>,
) -> StreamableHttpClientTransport<reqwest::Client> {
    let mut config = StreamableHttpClientTransportConfig::with_uri(server_config.endpoint.clone());

    if let Some(token) = resolve_bearer_token(server_config, connection_metadata).await {
        config = config.auth_header(token);
    }

    let client = if let Some(headers) = &server_config.headers {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(key.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                header_map.insert(name, val);
            }
        }
        reqwest::Client::builder()
            .default_headers(header_map)
            .build()
            .unwrap_or_default()
    } else {
        reqwest::Client::default()
    };

    StreamableHttpClientTransport::with_client(client, config)
}

fn client_info() -> ClientInfo {
    ClientInfo {
        protocol_version: Default::default(),
        capabilities: ClientCapabilities::default(),
        client_info: Implementation {
            name: "wacht-agent".to_string(),
            title: None,
            version: env!("CARGO_PKG_VERSION").to_string(),
            website_url: None,
            icons: None,
        },
    }
}

fn is_connection_usable(conn: &ActorMcpConnection) -> bool {
    let requires_user = conn
        .server
        .config
        .auth
        .as_ref()
        .map(|a| a.requires_user_connection())
        .unwrap_or(false);

    if !requires_user {
        return true;
    }

    let Some(meta) = &conn.connection_metadata else {
        return false;
    };

    meta.expires_at
        .map(|exp| exp > Utc::now())
        .unwrap_or(true)
}

async fn discover_tools_from_connection(
    conn: ActorMcpConnection,
    deployment_id: i64,
) -> Vec<AiTool> {
    if !is_connection_usable(&conn) {
        return Vec::new();
    }

    let server_id = conn.server.id;
    let transport = build_transport(&conn.server.config, conn.connection_metadata.as_ref()).await;

    let client = match timeout(Duration::from_secs(10), client_info().serve(transport)).await {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            tracing::warn!(server_id, "MCP tool discovery failed to connect: {}", e);
            return Vec::new();
        }
        Err(_) => {
            tracing::warn!(server_id, "MCP tool discovery timed out connecting");
            return Vec::new();
        }
    };

    let list_result =
        timeout(Duration::from_secs(15), client.list_tools(Default::default())).await;

    let _ = client.cancel().await;

    let tool_list = match list_result {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            tracing::warn!(server_id, "MCP list_tools failed: {}", e);
            return Vec::new();
        }
        Err(_) => {
            tracing::warn!(server_id, "MCP list_tools timed out");
            return Vec::new();
        }
    };

    tool_list
        .tools
        .into_iter()
        .map(|tool| {
            let input_schema: Option<Value> = serde_json::to_value(&tool.input_schema).ok();
            AiTool {
                id: mcp_tool_synthetic_id(server_id, &tool.name),
                name: mcp_tool_agent_name(server_id, &tool.name),
                description: tool.description.map(|d: std::borrow::Cow<'_, str>| d.into_owned()),
                tool_type: AiToolType::Mcp,
                deployment_id,
                requires_user_approval: false,
                configuration: AiToolConfiguration::Mcp(McpToolConfiguration {
                    mcp_server_id: server_id,
                    remote_tool_name: tool.name.to_string(),
                    input_schema,
                }),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            }
        })
        .collect()
}

pub async fn discover_mcp_tools_for_actor(
    connections: Vec<ActorMcpConnection>,
    deployment_id: i64,
) -> Vec<AiTool> {
    let per_server = futures::future::join_all(
        connections
            .into_iter()
            .map(|conn| discover_tools_from_connection(conn, deployment_id)),
    )
    .await;

    per_server.into_iter().flatten().collect()
}

impl ToolExecutor {
    pub(super) async fn execute_mcp_tool(
        &self,
        _tool: &AiTool,
        config: &McpToolConfiguration,
        execution_params: &Value,
        _filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        let conn = self
            .get_actor_mcp_connection(config.mcp_server_id)
            .await?
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "No active connection to MCP server {}",
                    config.mcp_server_id
                ))
            })?;

        let transport =
            build_transport(&conn.server.config, conn.connection_metadata.as_ref()).await;

        let client = timeout(Duration::from_secs(10), client_info().serve(transport))
            .await
            .map_err(|_| AppError::BadRequest("MCP connection timed out".to_string()))?
            .map_err(|e| AppError::BadRequest(format!("MCP connection failed: {}", e)))?;

        let arguments: Option<serde_json::Map<String, Value>> = if execution_params.is_null()
            || execution_params == &Value::Object(Default::default())
        {
            None
        } else {
            execution_params.as_object().cloned()
        };

        let result = timeout(
            Duration::from_secs(60),
            client.call_tool(CallToolRequestParam {
                name: config.remote_tool_name.clone().into(),
                arguments,
            }),
        )
        .await
        .map_err(|_| AppError::BadRequest("MCP tool call timed out".to_string()))?
        .map_err(|e| AppError::BadRequest(format!("MCP tool call failed: {}", e)))?;

        let _ = client.cancel().await;

        let content: Vec<Value> = result
            .content
            .iter()
            .filter_map(|item| serde_json::to_value(item).ok())
            .collect();

        Ok(serde_json::json!({
            "is_error": result.is_error.unwrap_or(false),
            "content": content,
        }))
    }

    async fn get_actor_mcp_connection(
        &self,
        mcp_server_id: i64,
    ) -> Result<Option<ActorMcpConnection>, AppError> {
        let thread = self.ctx.get_thread().await?;
        let connections = queries::GetActorMcpConnectionsQuery::new(
            self.ctx.agent.deployment_id,
            thread.actor_id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;

        let Some(mut conn) = connections
            .into_iter()
            .find(|c| c.server.id == mcp_server_id)
        else {
            return Ok(None);
        };

        if connection_needs_refresh(&conn) {
            if let Some(new_meta) = refresh_connection_metadata(&conn).await {
                if let Ok(meta_json) = serde_json::to_value(&new_meta) {
                    let _ = queries::UpdateActorMcpConnectionMetadataQuery::new(
                        self.ctx.agent.deployment_id,
                        thread.actor_id,
                        mcp_server_id,
                        meta_json,
                    )
                    .execute_with_db(self.ctx.app_state.db_router.writer())
                    .await;
                }
                conn.connection_metadata = Some(new_meta);
            }
        }

        if is_connection_usable(&conn) {
            Ok(Some(conn))
        } else {
            Ok(None)
        }
    }
}
