use crate::filesystem::{shell::ShellExecutor, AgentFilesystem};
use crate::tools::ToolExecutor;

use commands::EnsureExecutionTaskGraphCommand;
use common::error::AppError;
use dto::json::agent_executor::{ConversationInsights, ObjectiveDefinition};
use dto::json::StreamEvent;
use models::{AgentExecutionState, ConversationRecord, ExecutionContextStatus, MemoryRecord};
use models::{
    AiTool, AiToolConfiguration, AiToolType, InternalToolConfiguration,
    UseExternalServiceToolConfiguration, UseExternalServiceToolType,
};
use queries::{
    ListExecutionTaskEdgesQuery, ListExecutionTaskNodesQuery, ListReadyExecutionTaskNodesQuery,
};
use rmcp::{
    model::{ClientCapabilities, ClientInfo, Implementation},
    transport::{
        streamable_http_client::StreamableHttpClientTransportConfig, StreamableHttpClientTransport,
    },
    ServiceExt,
};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub enum ResumeContext {
    PlatformFunction(String, serde_json::Value),
    UserInput(String),
}

pub struct AgentExecutor {
    pub(super) ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
    pub(super) conversations: Vec<ConversationRecord>,
    pub(super) tool_executor: ToolExecutor,
    pub(super) channel: tokio::sync::mpsc::Sender<StreamEvent>,
    pub(super) memories: Vec<MemoryRecord>,
    pub(super) loaded_memory_ids: std::collections::HashSet<i64>,
    pub(super) user_request: String,
    pub(super) current_objective: Option<ObjectiveDefinition>,
    pub(super) conversation_insights: Option<ConversationInsights>,
    pub(super) system_instructions: Option<String>,
    pub(super) filesystem: AgentFilesystem,
    pub(super) shell: ShellExecutor,
    pub(super) current_iteration: usize,
    pub(super) deep_think_mode_active: bool,
    pub(super) deep_think_used: usize,
    pub(super) supervisor_mode_active: bool,
    pub(super) supervisor_task_board: Vec<serde_json::Value>,
    pub(super) task_graph_snapshot: Option<serde_json::Value>,
    pub(super) last_decision_signature: Option<String>,
    pub(super) repeated_decision_count: usize,
}

pub struct AgentExecutorBuilder {
    ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
    channel: tokio::sync::mpsc::Sender<StreamEvent>,
}

impl AgentExecutorBuilder {
    pub fn new(
        ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Self {
        Self { ctx, channel }
    }

    pub async fn build(self) -> Result<AgentExecutor, AppError> {
        let execution_context = self.ctx.clone();

        let tool_executor =
            ToolExecutor::new(execution_context.clone()).with_channel(self.channel.clone());
        let execution_id = self.ctx.app_state.sf.next_id()?.to_string();

        let filesystem = AgentFilesystem::new(
            &self.ctx.agent.deployment_id.to_string(),
            &self.ctx.context_id.to_string(),
            &execution_id,
        );

        if let Err(e) = filesystem.initialize().await {
            tracing::warn!("Failed to initialize agent filesystem: {}", e);
        }

        let shell = ShellExecutor::new(filesystem.execution_root());

        for kb in &self.ctx.agent.knowledge_bases {
            if let Err(e) = filesystem
                .link_knowledge_base(&kb.id.to_string(), &kb.name)
                .await
            {
                tracing::warn!(
                    "Failed to link knowledge base {} ({}): {}",
                    kb.name,
                    kb.id,
                    e
                );
            }
        }

        let internal_tools = super::tool_definitions::internal_tools();

        let context = execution_context.get_context().await?;

        // Get integration status from cached context
        let integration_status = execution_context.integration_status().await?;

        // Link teams activity directory if Teams is enabled
        let mut current_tools = self.ctx.agent.tools.clone();
        for (name, desc, tool_type, schema) in internal_tools {
            if !current_tools.iter().any(|t| t.name == name) {
                current_tools.push(AiTool {
                    id: -1,
                    name: name.to_string(),
                    description: Some(desc.to_string()),
                    tool_type: AiToolType::Internal,
                    deployment_id: self.ctx.agent.deployment_id,
                    configuration: AiToolConfiguration::Internal(InternalToolConfiguration {
                        tool_type,
                        input_schema: Some(schema),
                    }),
                    created_at: chrono::Utc::now(),
                    updated_at: chrono::Utc::now(),
                });
            }
        }

        if integration_status.teams_enabled {
            let teams_tools = super::tool_definitions::teams_tools();

            for (name, desc, service_type, schema) in teams_tools {
                if !current_tools.iter().any(|t| t.name == name) {
                    current_tools.push(AiTool {
                        id: -1,
                        name: name.to_string(),
                        description: Some(desc.to_string()),
                        tool_type: AiToolType::UseExternalService,
                        deployment_id: self.ctx.agent.deployment_id,
                        configuration: AiToolConfiguration::UseExternalService(
                            UseExternalServiceToolConfiguration {
                                service_type,
                                input_schema: Some(schema),
                            },
                        ),
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });
                }
            }
        }

        if integration_status.clickup_enabled {
            let clickup_tools = super::tool_definitions::clickup_tools();

            for (name, desc, service_type, schema) in clickup_tools {
                if !current_tools.iter().any(|t| t.name == name) {
                    current_tools.push(AiTool {
                        id: -1,
                        name: name.to_string(),
                        description: Some(desc.to_string()),
                        tool_type: AiToolType::UseExternalService,
                        deployment_id: self.ctx.agent.deployment_id,
                        configuration: AiToolConfiguration::UseExternalService(
                            UseExternalServiceToolConfiguration {
                                service_type,
                                input_schema: Some(schema),
                            },
                        ),
                        created_at: chrono::Utc::now(),
                        updated_at: chrono::Utc::now(),
                    });
                }
            }
        }

        if integration_status.mcp_enabled {
            let active_mcp_tools =
                build_context_group_mcp_tools(execution_context.as_ref(), &context).await?;
            for tool in active_mcp_tools {
                if !current_tools.iter().any(|t| t.name == tool.name) {
                    current_tools.push(tool);
                }
            }
        }

        if !current_tools
            .iter()
            .any(|t| t.name == "spawn_context_execution")
        {
            current_tools.push(AiTool {
                id: -1,
                name: "spawn_context_execution".to_string(),
                description: Some(
                    "Spawn a delegated agent execution. Choose `agent_name` as `self` or a configured sub-agent name. If `target_context_id` is provided, execution runs there; otherwise a temporary child context is created and inherits parent conversation history up to spawn time. Use this to delegate tasks, coordinate across contexts, or run isolated sub-tasks.".to_string()
                ),
                tool_type: AiToolType::UseExternalService,
                deployment_id: self.ctx.agent.deployment_id,
                configuration: AiToolConfiguration::UseExternalService(
                    UseExternalServiceToolConfiguration {
                        service_type: UseExternalServiceToolType::SpawnContextExecution,
                        input_schema: Some(crate::executor::tool_definitions::spawn_context_execution_schema()),
                    }
                ),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            });
        }

        let mut agent_with_tools = self.ctx.agent.clone();
        agent_with_tools.tools = current_tools.clone();

        let execution_context = execution_context.with_agent(agent_with_tools);

        let mut executor = AgentExecutor {
            ctx: execution_context,
            tool_executor,
            user_request: String::new(),
            channel: self.channel,
            memories: Vec::new(),
            loaded_memory_ids: std::collections::HashSet::new(),
            conversations: Vec::new(),
            current_objective: None,
            conversation_insights: None,
            system_instructions: None,
            filesystem,
            shell,
            current_iteration: 0,
            deep_think_mode_active: false,
            deep_think_used: 0,
            supervisor_mode_active: false,
            supervisor_task_board: Vec::new(),
            task_graph_snapshot: None,
            last_decision_signature: None,
            repeated_decision_count: 0,
        };

        executor.system_instructions = context.system_instructions.clone();

        if context.status == ExecutionContextStatus::WaitingForInput {
            if let Some(state) = context.execution_state {
                executor.restore_from_state(state)?;
            }
        }

        executor.ensure_task_graph_snapshot().await?;

        Ok(executor)
    }
}

impl AgentExecutor {
    pub(super) fn invalidate_task_graph_snapshot(&mut self) {
        self.task_graph_snapshot = None;
    }

    pub(super) fn render_task_graph_view(snapshot: &serde_json::Value) -> String {
        let graph_status = snapshot
            .get("graph")
            .and_then(|graph| graph.get("status"))
            .and_then(|status| status.as_str())
            .unwrap_or("unknown");

        let nodes = snapshot
            .get("nodes")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let edges = snapshot
            .get("edges")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let ready_node_ids = snapshot
            .get("ready_node_ids")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|value| value.as_str().map(|s| s.to_string()))
            .collect::<std::collections::HashSet<_>>();

        let mut dependency_map: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        for edge in edges {
            let Some(to_node_id) = edge.get("to_node_id").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(from_node_id) = edge.get("from_node_id").and_then(|value| value.as_str()) else {
                continue;
            };
            dependency_map
                .entry(to_node_id.to_string())
                .or_default()
                .push(from_node_id.to_string());
        }

        let mut lines = vec![format!("Graph status: {graph_status}")];

        let ready_lines = nodes
            .iter()
            .filter_map(|node| {
                let id = node.get("id")?.as_str()?;
                if !ready_node_ids.contains(id) {
                    return None;
                }
                let title = node
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or("Untitled");
                Some(format!("- {id} {title}"))
            })
            .collect::<Vec<_>>();

        if ready_lines.is_empty() {
            lines.push("Ready nodes: none".to_string());
        } else {
            lines.push("Ready nodes:".to_string());
            lines.extend(ready_lines);
        }

        lines.push("All nodes:".to_string());

        for node in nodes {
            let id = node
                .get("id")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let title = node
                .get("title")
                .and_then(|value| value.as_str())
                .unwrap_or("Untitled");
            let status = node
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");

            lines.push(format!("- {id} {title} [{status}]"));

            if let Some(depends_on) = dependency_map.get(id) {
                if !depends_on.is_empty() {
                    lines.push(format!("  depends_on: {}", depends_on.join(", ")));
                }
            }
        }

        lines.join("\n")
    }

    pub(super) async fn ensure_task_graph_snapshot(&mut self) -> Result<serde_json::Value, AppError> {
        if let Some(snapshot) = &self.task_graph_snapshot {
            let graph_status = snapshot
                .get("graph")
                .and_then(|graph| graph.get("status"))
                .and_then(|status| status.as_str());

            if !matches!(graph_status, Some("completed" | "failed" | "cancelled")) {
                return Ok(snapshot.clone());
            }

            self.task_graph_snapshot = None;
        }

        let graph = EnsureExecutionTaskGraphCommand::new(
            self.ctx.app_state.sf.next_id()? as i64,
            self.ctx.agent.deployment_id,
            self.ctx.context_id,
        )
        .execute_with_db(self.ctx.app_state.db_router.writer())
        .await?;

        let nodes = ListExecutionTaskNodesQuery::new(graph.id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        let edges = ListExecutionTaskEdgesQuery::new(graph.id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;
        let ready_nodes = ListReadyExecutionTaskNodesQuery::new(graph.id)
            .execute_with_db(self.ctx.app_state.db_router.writer())
            .await?;

        let snapshot = serde_json::json!({
            "graph": graph,
            "nodes": nodes,
            "edges": edges,
            "ready_node_ids": ready_nodes
                .iter()
                .map(|node| node.id.to_string())
                .collect::<Vec<_>>(),
        });

        self.task_graph_snapshot = Some(snapshot.clone());

        Ok(snapshot)
    }
}

fn json_type_to_schema_field_type(json_type: &str) -> String {
    match json_type {
        "string" => "STRING",
        "integer" => "INTEGER",
        "number" => "NUMBER",
        "boolean" => "BOOLEAN",
        "array" => "ARRAY",
        "object" => "OBJECT",
        _ => "STRING",
    }
    .to_string()
}

fn mcp_json_schema_to_fields(schema: &serde_json::Value) -> Vec<models::SchemaField> {
    let required: HashSet<String> = schema
        .get("required")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    schema
        .get("properties")
        .and_then(|v| v.as_object())
        .map(|properties| {
            properties
                .iter()
                .map(|(name, prop)| {
                    let field_type = prop
                        .get("type")
                        .and_then(|v| v.as_str())
                        .map(json_type_to_schema_field_type)
                        .unwrap_or_else(|| "STRING".to_string());

                    let items_type = prop
                        .get("items")
                        .and_then(|v| v.get("type"))
                        .and_then(|v| v.as_str())
                        .map(json_type_to_schema_field_type);

                    models::SchemaField {
                        name: name.clone(),
                        field_type,
                        required: required.contains(name),
                        description: prop
                            .get("description")
                            .and_then(|v| v.as_str())
                            .map(str::to_string),
                        items_type,
                    }
                })
                .collect()
        })
        .unwrap_or_else(|| {
            vec![models::SchemaField {
                name: "arguments".to_string(),
                field_type: "OBJECT".to_string(),
                required: false,
                description: Some("JSON object of arguments for this MCP tool.".to_string()),
                items_type: None,
            }]
        })
}

async fn mcp_auth_token_for_server(
    execution_context: &crate::execution_context::ExecutionContext,
    context_group: &str,
    server: &models::McpServer,
) -> Result<Option<String>, AppError> {
    match &server.config.auth {
        None => Ok(None),
        Some(models::McpAuthConfig::Token { auth_token }) => Ok(Some(auth_token.clone())),
        Some(models::McpAuthConfig::OAuthClientCredentials {
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
                    AppError::External("MCP OAuth token response missing access_token".to_string())
                })?;
            Ok(Some(access_token.to_string()))
        }
        Some(models::McpAuthConfig::OAuthAuthorizationCodePublicPkce { .. })
        | Some(models::McpAuthConfig::OAuthAuthorizationCodeConfidentialPkce { .. }) => {
            let metadata = queries::GetActiveAgentMcpServerConnectionMetadataQuery::new(
                execution_context.agent.deployment_id,
                execution_context.agent.id,
                context_group.to_string(),
                server.id,
            )
            .execute_with_db(execution_context.app_state.db_router.writer())
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

async fn list_mcp_tools_for_server(
    execution_context: &crate::execution_context::ExecutionContext,
    context_group: &str,
    server: &models::McpServer,
) -> Result<Vec<AiTool>, AppError> {
    let endpoint = server.config.endpoint.trim();
    if endpoint.is_empty() {
        return Ok(Vec::new());
    }

    let auth_token = mcp_auth_token_for_server(execution_context, context_group, server).await?;
    let transport_config = if let Some(token) = auth_token {
        StreamableHttpClientTransportConfig::with_uri(endpoint.to_string()).auth_header(token)
    } else {
        StreamableHttpClientTransportConfig::with_uri(endpoint.to_string())
    };
    let transport = StreamableHttpClientTransport::from_config(transport_config);
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

    let list = client
        .list_tools(Default::default())
        .await
        .map_err(|e| AppError::External(format!("MCP list_tools failed: {}", e)))?;

    if let Err(error) = client.cancel().await {
        tracing::warn!("Failed to close MCP client cleanly: {}", error);
    }

    let list_value = serde_json::to_value(list)?;
    let tool_entries = list_value
        .get("tools")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut dynamic_tools = Vec::new();
    for entry in tool_entries {
        let Some(tool_name) = entry.get("name").and_then(|v| v.as_str()) else {
            continue;
        };

        let input_schema = entry
            .get("input_schema")
            .or_else(|| entry.get("inputSchema"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let schema_fields = mcp_json_schema_to_fields(&input_schema);
        let alias = super::tool_definitions::mcp_dynamic_tool_name(&server.name, tool_name);
        let description = entry
            .get("description")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| {
                Some(format!(
                    "MCP tool '{}' from server '{}'.",
                    tool_name, server.name
                ))
            });

        dynamic_tools.push(AiTool {
            id: -1,
            name: alias,
            description,
            tool_type: AiToolType::UseExternalService,
            deployment_id: server.deployment_id,
            configuration: AiToolConfiguration::UseExternalService(
                UseExternalServiceToolConfiguration {
                    service_type: UseExternalServiceToolType::McpCallTool,
                    input_schema: Some(schema_fields),
                },
            ),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        });
    }

    Ok(dynamic_tools)
}

async fn build_context_group_mcp_tools(
    execution_context: &crate::execution_context::ExecutionContext,
    context: &models::AgentExecutionContext,
) -> Result<Vec<AiTool>, AppError> {
    let attached_servers = queries::GetAgentMcpServersQuery::new(
        execution_context.agent.deployment_id,
        execution_context.agent.id,
    )
    .execute_with_db(execution_context.app_state.db_router.writer())
    .await?;

    let Some(context_group) = context.context_group.as_ref() else {
        return Ok(Vec::new());
    };

    let active_server_ids = queries::GetActiveAgentMcpServerIdsForContextQuery::new(
        execution_context.agent.deployment_id,
        execution_context.agent.id,
        context_group.clone(),
    )
    .execute_with_db(execution_context.app_state.db_router.writer())
    .await?;

    let active_id_set: HashSet<i64> = active_server_ids.into_iter().collect();
    let active_servers: Vec<_> = attached_servers
        .into_iter()
        .filter(|server| {
            let requires_user_connection = server
                .config
                .auth
                .as_ref()
                .map(|auth| auth.requires_user_connection())
                .unwrap_or(false);

            !requires_user_connection || active_id_set.contains(&server.id)
        })
        .collect();

    let mut mcp_tools = Vec::new();
    for server in active_servers {
        match list_mcp_tools_for_server(execution_context, context_group, &server).await {
            Ok(mut tools) => mcp_tools.append(&mut tools),
            Err(error) => tracing::warn!(
                "Failed to list MCP tools for server '{}': {}",
                server.name,
                error
            ),
        }
    }

    Ok(mcp_tools)
}

impl AgentExecutor {
    pub async fn new(
        ctx: std::sync::Arc<crate::execution_context::ExecutionContext>,
        channel: tokio::sync::mpsc::Sender<StreamEvent>,
    ) -> Result<Self, AppError> {
        AgentExecutorBuilder::new(ctx, channel).build().await
    }

    pub(super) fn restore_from_state(
        &mut self,
        state: AgentExecutionState,
    ) -> Result<(), AppError> {
        if let Some(objective) = state.current_objective {
            self.current_objective = serde_json::from_value(objective).ok();
        }

        if let Some(insights) = state.conversation_insights {
            self.conversation_insights = serde_json::from_value(insights).ok();
        }

        self.deep_think_mode_active = state.deep_think_mode_active;
        self.deep_think_used = state.deep_think_used;
        self.supervisor_mode_active = state.supervisor_mode_active;
        self.supervisor_task_board = state.supervisor_task_board;

        Ok(())
    }

    pub(super) fn is_supervisor_mode(&self) -> bool {
        self.supervisor_mode_active
    }

    pub(super) fn supervisor_allowed_tool(tool_name: &str) -> bool {
        matches!(
            tool_name,
            "spawn_context_execution"
                | "get_child_status"
                | "get_completion_summary"
                | "get_child_messages"
                | "spawn_control"
                | "update_task_board"
                | "exit_supervisor_mode"
                | "sleep"
        )
    }
}
