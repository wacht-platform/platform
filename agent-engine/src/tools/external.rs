use super::ToolExecutor;
use crate::filesystem::AgentFilesystem;
use crate::swarm;
use base64::Engine;
use common::error::AppError;
use flate2::read::GzDecoder;
use models::{
    AiTool, McpAuthConfig, McpServerConfig, UseExternalServiceToolConfiguration,
    UseExternalServiceToolType,
};
use rmcp::{
    model::{CallToolRequestParam, ClientCapabilities, ClientInfo, Implementation},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceExt,
};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;
use std::io::Read;

#[derive(Clone, Copy)]
enum ClickUpAction {
    GetCurrentUser,
    GetTeams,
    GetSpaces,
    GetSpaceLists,
    GetTask,
    GetTasks,
    SearchTasks,
    CreateTask,
    CreateList,
    UpdateTask,
    AddComment,
    AddAttachment,
}

#[derive(Clone, Copy)]
enum TeamsAction {
    ListUsers,
    SearchUsers,
    ListMessages,
    GetMeetingRecording,
    AnalyzeMeeting,
}

impl TeamsAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::ListUsers => "list_users",
            Self::SearchUsers => "search_users",
            Self::ListMessages => "list_messages",
            Self::GetMeetingRecording => "get_meeting_recording",
            Self::AnalyzeMeeting => "analyze_meeting",
        }
    }
}

#[derive(Clone, Copy)]
enum WhatsAppAction {
    SendMessage,
    GetMessage,
    MarkRead,
}

#[derive(Clone, Copy)]
enum McpAction {
    CallTool,
}

impl McpAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::CallTool => "call_tool",
        }
    }
}

#[derive(Debug, Deserialize)]
struct TeamIdParams {
    team_id: String,
}

#[derive(Debug, Deserialize)]
struct SpaceIdParams {
    space_id: String,
}

#[derive(Debug, Deserialize)]
struct TaskIdParams {
    task_id: String,
}

#[derive(Debug, Deserialize)]
struct ListIdParams {
    list_id: String,
}

#[derive(Debug, Deserialize)]
struct ClickUpAddAttachmentParams {
    task_id: String,
    filename: Option<String>,
    mime_type: Option<String>,
    file_data: String,
}

#[derive(Debug, Deserialize)]
struct ClickUpFileAttachmentParams {
    task_id: String,
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct WhatsAppSendMessageParams {
    to: String,
    message: String,
}

#[derive(Debug, Deserialize)]
struct WhatsAppMessageIdParams {
    message_id: String,
}

#[derive(Debug, Deserialize)]
struct TeamsSaveAttachmentParams {
    attachment_url: String,
    filename: String,
}

#[derive(Debug, Deserialize)]
struct TeamsListContextsParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct McpCallToolParams {
    server_name: Option<String>,
    tool_name: String,
    arguments: Option<serde_json::Value>,
}

#[derive(Debug)]
struct PrefixedMcpToolAlias {
    server_slug: String,
    tool_slug: String,
}

fn parse_external_params<T: DeserializeOwned>(
    execution_params: &Value,
    tool_name: &str,
) -> Result<T, AppError> {
    let normalized = if execution_params.is_null() {
        serde_json::json!({})
    } else {
        execution_params.clone()
    };

    serde_json::from_value::<T>(normalized)
        .map_err(|e| AppError::BadRequest(format!("Invalid {tool_name} params: {e}")))
}

impl ToolExecutor {
    fn mcp_server_requires_connection(server: &models::McpServer) -> bool {
        server
            .config
            .auth
            .as_ref()
            .map(|auth| auth.requires_user_connection())
            .unwrap_or(false)
    }

    fn mcp_name_slug(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        let mut prev_underscore = false;
        for ch in value.chars() {
            let lower = ch.to_ascii_lowercase();
            let is_valid = lower.is_ascii_alphanumeric();
            if is_valid {
                out.push(lower);
                prev_underscore = false;
            } else if !prev_underscore {
                out.push('_');
                prev_underscore = true;
            }
        }

        out.trim_matches('_').to_string()
    }

    fn parse_prefixed_mcp_tool_alias(tool_name: &str) -> Option<PrefixedMcpToolAlias> {
        let without_prefix = tool_name.strip_prefix("mcp__")?;
        let mut parts = without_prefix.splitn(2, "__");
        let server_slug = parts.next()?.to_string();
        let tool_slug = parts.next()?.to_string();
        if server_slug.is_empty() || tool_slug.is_empty() {
            return None;
        }
        Some(PrefixedMcpToolAlias {
            server_slug,
            tool_slug,
        })
    }

    async fn active_mcp_servers_for_context(&self) -> Result<Vec<models::McpServer>, AppError> {
        let attached_servers =
            queries::GetAgentMcpServersQuery::new(self.ctx.agent.deployment_id, self.ctx.agent.id)
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?;

        let context = self.ctx.get_context().await?;
        let Some(context_group) = context.context_group else {
            return Ok(Vec::new());
        };

        let active_server_ids = queries::GetActiveAgentMcpServerIdsForContextQuery::new(
            self.ctx.agent.deployment_id,
            self.ctx.agent.id,
            context_group,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;

        Ok(attached_servers
            .into_iter()
            .filter(|server| {
                !Self::mcp_server_requires_connection(server)
                    || active_server_ids.iter().any(|id| *id == server.id)
            })
            .collect())
    }

    async fn mcp_auth_token_for_server(
        &self,
        server: &models::McpServer,
    ) -> Result<Option<String>, AppError> {
        match &server.config.auth {
            None => Ok(None),
            Some(McpAuthConfig::Token { auth_token }) => Ok(Some(auth_token.clone())),
            Some(McpAuthConfig::OAuthClientCredentials {
                client_id,
                client_secret,
                token_url,
                scopes,
            }) => {
                let token_endpoint = token_url.as_ref().ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "MCP server '{}' is missing token_url for client credentials auth",
                        server.name
                    ))
                })?;
                let mut form = vec![("grant_type", "client_credentials".to_string())];
                if !scopes.is_empty() {
                    form.push(("scope", scopes.join(" ")));
                }
                let response = reqwest::Client::new()
                    .post(token_endpoint)
                    .basic_auth(client_id, Some(client_secret))
                    .form(&form)
                    .send()
                    .await
                    .map_err(|e| {
                        AppError::External(format!(
                            "Failed to request MCP OAuth client-credentials token: {}",
                            e
                        ))
                    })?;
                if !response.status().is_success() {
                    return Err(AppError::External(format!(
                        "MCP OAuth token endpoint returned {}",
                        response.status()
                    )));
                }
                let payload: serde_json::Value = response.json().await.map_err(|e| {
                    AppError::External(format!("Invalid MCP OAuth token response payload: {}", e))
                })?;
                let access_token = payload
                    .get("access_token")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AppError::External(
                            "MCP OAuth token response missing access_token".to_string(),
                        )
                    })?;
                Ok(Some(access_token.to_string()))
            }
            Some(McpAuthConfig::OAuthAuthorizationCodePublicPkce { .. })
            | Some(McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce { .. }) => {
                let context = self.ctx.get_context().await?;
                let context_group = context.context_group.ok_or_else(|| {
                    AppError::BadRequest("No context group found for MCP command".to_string())
                })?;
                let metadata = queries::GetActiveAgentMcpServerConnectionMetadataQuery::new(
                    self.ctx.agent.deployment_id,
                    self.ctx.agent.id,
                    context_group,
                    server.id,
                )
                .execute_with_db(self.ctx.app_state.db_router.writer())
                .await?
                .ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "MCP server '{}' is not connected for this context group",
                        server.name
                    ))
                })?;
                Ok(Some(metadata.access_token))
            }
        }
    }

    pub(super) async fn execute_external_service_tool(
        &self,
        tool: &AiTool,
        config: &UseExternalServiceToolConfiguration,
        execution_params: &Value,
        context_title: &str,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        match config.service_type {
            UseExternalServiceToolType::TeamsListUsers => {
                self.execute_teams_command(
                    tool,
                    TeamsAction::ListUsers,
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsSearchUsers => {
                self.execute_teams_command(
                    tool,
                    TeamsAction::SearchUsers,
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsSendContextMessage => {
                Err(AppError::BadRequest(
                    "teams_send_context_message is deprecated. Use spawn_context_execution with `target_context_id` and `instructions`."
                        .to_string(),
                ))
            }
            UseExternalServiceToolType::TeamsListMessages => {
                self.execute_teams_command(
                    tool,
                    TeamsAction::ListMessages,
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsGetMeetingRecording => {
                self.execute_teams_command(
                    tool,
                    TeamsAction::GetMeetingRecording,
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsTranscribeMeeting => {
                self.execute_teams_command(
                    tool,
                    TeamsAction::AnalyzeMeeting,
                    execution_params,
                    context_title,
                )
                .await
            }
            UseExternalServiceToolType::TeamsSaveAttachment => {
                self.execute_teams_save_attachment(tool, execution_params)
                    .await
            }
            UseExternalServiceToolType::TeamsListContexts => {
                self.execute_teams_list_conversations(execution_params).await
            }
            UseExternalServiceToolType::SpawnContextExecution => {
                let request = serde_json::from_value::<swarm::TriggerContextRequest>(
                    execution_params.clone(),
                )
                .map_err(|e| {
                    AppError::BadRequest(format!(
                        "Invalid spawn_context_execution params: {}",
                        e
                    ))
                })?;
                swarm::relay_to_context(self.ctx.clone(), &tool.name, request).await
            }
            UseExternalServiceToolType::ClickUpCreateTask => {
                self.execute_clickup_command(tool, ClickUpAction::CreateTask, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpCreateList => {
                self.execute_clickup_command(tool, ClickUpAction::CreateList, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpUpdateTask => {
                self.execute_clickup_command(tool, ClickUpAction::UpdateTask, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpAddComment => {
                self.execute_clickup_command(tool, ClickUpAction::AddComment, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetTask => {
                self.execute_clickup_command(tool, ClickUpAction::GetTask, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetSpaceLists => {
                self.execute_clickup_command(tool, ClickUpAction::GetSpaceLists, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetSpaces => {
                self.execute_clickup_command(tool, ClickUpAction::GetSpaces, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetTeams => {
                self.execute_clickup_command(tool, ClickUpAction::GetTeams, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetCurrentUser => {
                self.execute_clickup_command(tool, ClickUpAction::GetCurrentUser, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpGetTasks => {
                self.execute_clickup_command(tool, ClickUpAction::GetTasks, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpSearchTasks => {
                self.execute_clickup_command(tool, ClickUpAction::SearchTasks, execution_params)
                    .await
            }
            UseExternalServiceToolType::ClickUpTaskAddAttachment => {
                self.execute_clickup_add_attachment(tool, execution_params, filesystem)
                    .await
            }
            UseExternalServiceToolType::McpCallTool => {
                self.execute_mcp_command(tool, McpAction::CallTool, execution_params)
                    .await
            }
            UseExternalServiceToolType::WhatsAppSendMessage => {
                self.execute_whatsapp_command(WhatsAppAction::SendMessage, execution_params)
                    .await
            }
            UseExternalServiceToolType::WhatsAppGetMessage => {
                self.execute_whatsapp_command(WhatsAppAction::GetMessage, execution_params)
                    .await
            }
            UseExternalServiceToolType::WhatsAppMarkRead => {
                self.execute_whatsapp_command(WhatsAppAction::MarkRead, execution_params)
                    .await
            }
        }
    }

    async fn execute_clickup_command(
        &self,
        tool: &AiTool,
        action: ClickUpAction,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let client = self.ctx.get_clickup_client().await?;

        let result = match action {
            ClickUpAction::GetCurrentUser => client.get_current_user().await?,
            ClickUpAction::GetTeams => client.get_teams().await?,
            ClickUpAction::GetSpaces => {
                let params: TeamIdParams = parse_external_params(execution_params, "get_spaces")?;
                client
                    .get_spaces(params.team_id.trim(), execution_params)
                    .await?
            }
            ClickUpAction::GetSpaceLists => {
                let params: SpaceIdParams =
                    parse_external_params(execution_params, "get_space_lists")?;
                client.get_space_lists(params.space_id.trim()).await?
            }
            ClickUpAction::GetTask => {
                let params: TaskIdParams = parse_external_params(execution_params, "get_task")?;
                client.get_task(params.task_id.trim()).await?
            }
            ClickUpAction::GetTasks => {
                let params: ListIdParams = parse_external_params(execution_params, "get_tasks")?;
                client
                    .get_tasks(params.list_id.trim(), execution_params)
                    .await?
            }
            ClickUpAction::SearchTasks => {
                let params: TeamIdParams = parse_external_params(execution_params, "search_tasks")?;
                client
                    .search_tasks(params.team_id.trim(), execution_params)
                    .await?
            }
            ClickUpAction::CreateTask => {
                let params: SpaceIdParams = parse_external_params(execution_params, "create_task")?;
                client
                    .create_task(params.space_id.trim(), execution_params)
                    .await?
            }
            ClickUpAction::CreateList => {
                let params: SpaceIdParams = parse_external_params(execution_params, "create_list")?;
                client
                    .create_list(params.space_id.trim(), execution_params)
                    .await?
            }
            ClickUpAction::UpdateTask => {
                let params: TaskIdParams = parse_external_params(execution_params, "update_task")?;
                client
                    .update_task(params.task_id.trim(), execution_params)
                    .await?
            }
            ClickUpAction::AddComment => {
                let params: TaskIdParams = parse_external_params(execution_params, "add_comment")?;
                client
                    .add_comment(params.task_id.trim(), execution_params)
                    .await?
            }
            ClickUpAction::AddAttachment => {
                let params: ClickUpAddAttachmentParams =
                    parse_external_params(execution_params, "add_attachment")?;
                let filename = params.filename.as_deref().unwrap_or("attachment");
                let mime_type = params
                    .mime_type
                    .as_deref()
                    .unwrap_or("application/octet-stream");

                let file_data = base64::engine::general_purpose::STANDARD
                    .decode(params.file_data)
                    .map_err(|e| {
                        AppError::BadRequest(format!("Invalid base64 file data: {}", e))
                    })?;

                client
                    .add_attachment(params.task_id.trim(), filename, mime_type, file_data)
                    .await?
            }
        };

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "result": result
        }))
    }

    async fn execute_clickup_add_attachment(
        &self,
        tool: &AiTool,
        execution_params: &Value,
        filesystem: &AgentFilesystem,
    ) -> Result<Value, AppError> {
        let params: ClickUpFileAttachmentParams =
            parse_external_params(execution_params, "clickup_task_add_attachment")?;

        let file_bytes = filesystem.read_file_bytes(&params.file_path).await?;

        let filename = std::path::Path::new(&params.file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment");

        let extension = std::path::Path::new(&params.file_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let mime_type = match extension.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "pdf" => "application/pdf",
            "txt" => "text/plain",
            "csv" => "text/csv",
            "json" => "application/json",
            "xml" => "application/xml",
            "zip" => "application/zip",
            "doc" => "application/msword",
            "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "xls" => "application/vnd.ms-excel",
            "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            _ => "application/octet-stream",
        };

        let file_base64 = base64::engine::general_purpose::STANDARD.encode(&file_bytes);
        let file_size = file_bytes.len();

        let enhanced_params = serde_json::json!({
            "task_id": params.task_id,
            "filename": filename,
            "mime_type": mime_type,
            "file_data": file_base64
        });

        let mut result = self
            .execute_clickup_command(tool, ClickUpAction::AddAttachment, &enhanced_params)
            .await?;

        if let Some(obj) = result.as_object_mut() {
            obj.insert("uploaded_file".to_string(), serde_json::json!(filename));
            obj.insert("file_size_bytes".to_string(), serde_json::json!(file_size));
        }

        Ok(result)
    }

    async fn execute_whatsapp_command(
        &self,
        action: WhatsAppAction,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        match action {
            WhatsAppAction::SendMessage => {
                let params: WhatsAppSendMessageParams =
                    parse_external_params(execution_params, "whatsapp_send_message")?;
                let _ = params.message;

                let message_id = self.ctx.app_state.sf.next_id().unwrap_or(0);
                Ok(serde_json::json!({
                    "success": true,
                    "message": "WhatsApp message sent (placeholder)",
                    "to": params.to,
                    "message_id": format!("wa_{}", message_id),
                }))
            }
            WhatsAppAction::GetMessage => {
                let params: WhatsAppMessageIdParams =
                    parse_external_params(execution_params, "whatsapp_get_message")?;

                Ok(serde_json::json!({
                    "message_id": params.message_id,
                    "from": "1234567890",
                    "to": "0987654321",
                    "text": "Sample message",
                    "timestamp": chrono::Utc::now().to_rfc3339(),
                }))
            }
            WhatsAppAction::MarkRead => {
                let params: WhatsAppMessageIdParams =
                    parse_external_params(execution_params, "whatsapp_mark_read")?;

                Ok(serde_json::json!({
                    "success": true,
                    "message_id": params.message_id,
                    "status": "read",
                }))
            }
        }
    }

    fn select_mcp_server<'a>(
        &self,
        mcp_servers: &'a [models::McpServer],
        requested_server: Option<&str>,
    ) -> Result<&'a models::McpServer, AppError> {
        if let Some(server_name) = requested_server {
            let normalized = server_name.trim().to_ascii_lowercase();
            return mcp_servers
                .iter()
                .find(|server| server.name.to_ascii_lowercase() == normalized)
                .ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "MCP server '{}' is not attached to this agent",
                        server_name
                    ))
                });
        }

        if mcp_servers.len() == 1 {
            return Ok(&mcp_servers[0]);
        }

        let available = mcp_servers
            .iter()
            .map(|server| server.name.clone())
            .collect::<Vec<_>>()
            .join(", ");

        Err(AppError::BadRequest(format!(
            "Multiple MCP servers are active. Provide 'server_name'. Available: {}",
            available
        )))
    }

    fn select_mcp_server_by_slug<'a>(
        &self,
        mcp_servers: &'a [models::McpServer],
        requested_server_slug: &str,
    ) -> Result<&'a models::McpServer, AppError> {
        mcp_servers
            .iter()
            .find(|server| Self::mcp_name_slug(&server.name) == requested_server_slug)
            .ok_or_else(|| {
                AppError::BadRequest(format!(
                    "MCP server with slug '{}' is not active in this context group",
                    requested_server_slug
                ))
            })
    }

    fn mcp_endpoint_from_config(config: &McpServerConfig) -> Result<String, AppError> {
        let trimmed = config.endpoint.trim();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest(
                "MCP integration config is missing endpoint/url".to_string(),
            ));
        }

        Ok(trimmed.to_string())
    }

    async fn execute_mcp_command(
        &self,
        tool: &AiTool,
        action: McpAction,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let mcp_servers = self.active_mcp_servers_for_context().await?;

        if mcp_servers.is_empty() {
            return Err(AppError::BadRequest(
                "No active MCP server connection for this context group".to_string(),
            ));
        }

        let prefixed_alias = Self::parse_prefixed_mcp_tool_alias(&tool.name);
        let (selected_server, call_params) = match action {
            McpAction::CallTool => {
                if let Some(alias) = prefixed_alias {
                    let server =
                        self.select_mcp_server_by_slug(&mcp_servers, &alias.server_slug)?;
                    let parsed = McpCallToolParams {
                        server_name: Some(server.name.clone()),
                        tool_name: alias.tool_slug,
                        arguments: execution_params
                            .as_object()
                            .map(|_| execution_params.clone()),
                    };
                    (server, Some(parsed))
                } else {
                    let parsed: McpCallToolParams =
                        parse_external_params(execution_params, "mcp_call_tool")?;
                    let server =
                        self.select_mcp_server(&mcp_servers, parsed.server_name.as_deref())?;
                    (server, Some(parsed))
                }
            }
        };

        let endpoint = Self::mcp_endpoint_from_config(&selected_server.config)?;
        let auth_token = self.mcp_auth_token_for_server(selected_server).await?;
        let transport = if let Some(token) = auth_token {
            let transport_config =
                StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token);
            StreamableHttpClientTransport::from_config(transport_config)
        } else {
            StreamableHttpClientTransport::from_uri(endpoint)
        };
        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "wacht-agent-engine".to_string(),
                title: None,
                version: env!("CARGO_PKG_VERSION").to_string(),
                website_url: None,
                icons: None,
            },
        };
        let client = client_info
            .serve(transport)
            .await
            .map_err(|e| AppError::External(format!("Failed to connect MCP server: {}", e)))?;

        let response_data = match action {
            McpAction::CallTool => {
                let params = call_params.ok_or_else(|| {
                    AppError::BadRequest("Missing call params for mcp_call_tool".to_string())
                })?;
                let mut resolved_tool_name = params.tool_name.clone();
                if Self::parse_prefixed_mcp_tool_alias(&tool.name).is_some() {
                    let listed = client
                        .list_tools(Default::default())
                        .await
                        .map_err(|e| AppError::External(format!("MCP list_tools failed: {}", e)))?;
                    let listed_value = serde_json::to_value(listed)?;
                    let tool_entries = listed_value
                        .get("tools")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let matched = tool_entries.iter().find_map(|entry| {
                        let name = entry.get("name").and_then(|v| v.as_str())?;
                        let slug = Self::mcp_name_slug(name);
                        if slug == params.tool_name {
                            Some(name.to_string())
                        } else {
                            None
                        }
                    });
                    resolved_tool_name = matched.ok_or_else(|| {
                        AppError::BadRequest(format!(
                            "Could not resolve MCP tool '{}' on server '{}'",
                            params.tool_name, selected_server.name
                        ))
                    })?;
                }

                let arguments = params.arguments.unwrap_or_else(|| serde_json::json!({}));

                let call_result = client
                    .call_tool(CallToolRequestParam {
                        name: resolved_tool_name.into(),
                        arguments: arguments.as_object().cloned(),
                    })
                    .await
                    .map_err(|e| AppError::External(format!("MCP call_tool failed: {}", e)))?;
                serde_json::to_value(call_result)?
            }
        };

        if let Err(error) = client.cancel().await {
            tracing::warn!("Failed to close MCP client cleanly: {}", error);
        }

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "action": action.as_str(),
            "server_name": selected_server.name,
            "result": response_data
        }))
    }

    async fn execute_teams_command(
        &self,
        tool: &AiTool,
        action: TeamsAction,
        execution_params: &Value,
        _context_title: &str,
    ) -> Result<Value, AppError> {
        let target_context_id = execution_params.get("context_id").and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        });

        let (context, effective_context_id) = if let Some(target_id) = target_context_id {
            let ctx = self.ctx.get_context_by_id(target_id).await.map_err(|_| {
                AppError::BadRequest(format!("Context {} not found or not accessible", target_id))
            })?;
            (ctx, target_id)
        } else {
            (self.ctx.get_context().await?, self.ctx.context_id)
        };

        if target_context_id.is_some() && context.source.as_deref() != Some("teams") {
            return Err(AppError::BadRequest(
                "Target context is not a Teams context".to_string(),
            ));
        }

        let context_group = context.context_group.ok_or_else(|| {
            AppError::BadRequest("No context group found for Teams command".to_string())
        })?;

        let payload = serde_json::json!({
            "deployment_id": self.ctx.agent.deployment_id.to_string(),
            "context_id": effective_context_id.to_string(),
            "context_group": context_group,
            "agent_id": self.ctx.agent.id.to_string(),
            "action": action.as_str(),
            "params": execution_params
        });

        let response = self
            .app_state()
            .nats_client
            .request(
                "integrations.teams.command".to_string(),
                serde_json::to_vec(&payload)?.into(),
            )
            .await
            .map_err(|e| AppError::External(format!("Teams integration request failed: {}", e)))?;

        let payload = response.payload.clone();
        let is_gzipped = payload.len() > 2 && payload[0] == 0x1f && payload[1] == 0x8b;

        let response_data: Value = if is_gzipped {
            let mut decoder = GzDecoder::new(&payload[..]);
            let mut decoded_string = String::new();
            decoder
                .read_to_string(&mut decoded_string)
                .map_err(|e| AppError::External(format!("Decompression failed: {}", e)))?;
            serde_json::from_str(&decoded_string)?
        } else {
            serde_json::from_slice(&payload)?
        };

        if response_data.get("success") == Some(&serde_json::json!(false)) {
            let error_msg = response_data
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Unknown error from Teams integration");
            return Ok(serde_json::json!({
                "success": false,
                "tool": tool.name,
                "error": error_msg
            }));
        }

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "result": response_data
        }))
    }

    async fn execute_teams_save_attachment(
        &self,
        tool: &AiTool,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: TeamsSaveAttachmentParams =
            parse_external_params(execution_params, "teams_save_attachment")?;

        let context = self.ctx.get_context().await?;
        let context_group = context
            .context_group
            .ok_or_else(|| AppError::BadRequest("No context group found".to_string()))?;

        let payload = serde_json::json!({
            "deployment_id": self.agent().deployment_id.to_string(),
            "context_id": self.context_id().to_string(),
            "context_group": context_group,
            "agent_id": self.agent().id.to_string(),
            "action": "download_attachment",
            "params": { "attachment_url": params.attachment_url }
        });

        let response = self
            .app_state()
            .nats_client
            .request(
                "integrations.teams.command".to_string(),
                serde_json::to_vec(&payload)?.into(),
            )
            .await
            .map_err(|e| AppError::External(format!("Failed to download attachment: {}", e)))?;

        let response_data: Value = serde_json::from_slice(&response.payload)?;

        if response_data.get("success") != Some(&serde_json::json!(true)) {
            let error_msg = response_data
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("Failed to download attachment");
            return Ok(serde_json::json!({
                "success": false,
                "tool": tool.name,
                "error": error_msg
            }));
        }

        let base64_data = response_data
            .get("data")
            .and_then(|d| d.as_str())
            .ok_or_else(|| AppError::Internal("No data in download response".to_string()))?;

        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64_data)
            .map_err(|e| AppError::Internal(format!("Invalid base64 data: {}", e)))?;

        let execution_id = self
            .app_state()
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate ID: {}", e)))?
            .to_string();

        let filesystem = AgentFilesystem::new(
            &self.agent().deployment_id.to_string(),
            &self.context_id().to_string(),
            &execution_id,
        );

        let clean_filename = std::path::Path::new(&params.filename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("attachment");

        let saved_path = filesystem.save_upload(clean_filename, &bytes).await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "result": {
                "saved": true,
                "path": saved_path,
                "description": response_data.get("description")
            }
        }))
    }

    async fn execute_teams_list_conversations(
        &self,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let params: TeamsListContextsParams =
            parse_external_params(execution_params, "teams_list_conversations")?;
        let limit = params.limit.unwrap_or(25) as u32;
        let offset = params.offset.unwrap_or(0) as u32;

        let current_context = self.ctx.get_context().await?;

        let context_group = current_context.context_group.ok_or_else(|| {
            AppError::BadRequest(
                "No context group found - this tool requires a Teams context".to_string(),
            )
        })?;

        let contexts = queries::ListExecutionContextsQuery::new(self.ctx.agent.deployment_id)
            .with_source_filter("teams".to_string())
            .with_context_group_filter(context_group.clone())
            .with_limit(limit)
            .with_offset(offset)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        let result: Vec<serde_json::Value> = contexts
            .iter()
            .map(|ctx| {
                serde_json::json!({
                    "context_id": ctx.id.to_string(),
                    "title": ctx.title,
                    "status": ctx.status.to_string(),
                    "last_activity": ctx.last_activity_at.to_rfc3339(),
                    "is_current": ctx.id == self.context_id()
                })
            })
            .collect();

        Ok(serde_json::json!({
            "contexts": result,
            "total": result.len(),
            "offset": offset,
            "context_group": context_group
        }))
    }
}
