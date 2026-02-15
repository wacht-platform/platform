use commands::{Command, TriggerWebhookEventCommand};
use common::state::AppState;

async fn create_virtual_system_agent(
    deployment_id: i64,
    name: &str,
    description: &str,
    virtual_id: i64,
    app_state: &AppState,
) -> Result<models::AiAgentWithFeatures, anyhow::Error> {
    use queries::{GetAiToolsQuery, Query};

    // Get all tools for the deployment - filter by integration type in memory
    let all_tools = GetAiToolsQuery::new(deployment_id)
        .execute(app_state)
        .await?;

    // Determine integration type from agent name
    let integration_filter = match name {
        "Teams Agent" => "teams",
        "ClickUp Agent" => "clickup",
        "WhatsApp Agent" => "whatsapp",
        _ => "",
    };

    // Filter tools by checking their configuration
    let tools: Vec<models::AiTool> = all_tools
        .into_iter()
        .filter(|t| {
            if let models::AiToolConfiguration::UseExternalService(config) = &t.configuration {
                config
                    .service_type
                    .integration_type()
                    .map(|it| it.eq_ignore_ascii_case(integration_filter))
                    .unwrap_or(false)
            } else {
                false
            }
        })
        .map(|t| models::AiTool {
            id: t.id,
            created_at: t.created_at,
            updated_at: t.updated_at,
            name: t.name,
            description: t.description,
            tool_type: t.tool_type,
            deployment_id: t.deployment_id,
            configuration: t.configuration,
        })
        .collect();

    Ok(models::AiAgentWithFeatures {
        id: virtual_id,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        name: name.to_string(),
        description: Some(description.to_string()),
        deployment_id,
        configuration: serde_json::json!({
            "tool_ids": tools.iter().map(|t| t.id).collect::<Vec<_>>(),
            "knowledge_base_ids": [],
            "integration_ids": [],
            "quick_questions": [],
        }),
        tools,
        knowledge_bases: vec![],
        integrations: vec![],
        sub_agents: None,
        spawn_config: Some(models::SpawnConfig {
            max_parallel_children: Some(1),
            default_timeout_secs: Some(120),
            allow_fork: Some(false),
            allow_exec: Some(false),
        }),
    })
}

pub async fn process_agent_execution(
    app_state: &AppState,
    request: dto::json::AgentExecutionRequest,
) -> Result<String, anyhow::Error> {
    use agent_engine::{AgentHandler, ExecutionRequest};
    use dto::json::AgentExecutionType;
    use queries::{GetAiAgentByIdWithFeatures, GetAiAgentByNameWithFeatures, Query};

    let agent_identifier = request
        .agent_id
        .as_ref()
        .map(|id| id.to_string())
        .or(request.agent_name.clone())
        .unwrap_or_else(|| "unknown".to_string());

    // Parse string IDs to i64
    let deployment_id: i64 = request
        .deployment_id
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid deployment_id '{}': {}", request.deployment_id, e))?;
    let context_id: i64 = request
        .context_id
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid context_id '{}': {}", request.context_id, e))?;

    tracing::info!(
        "Processing agent '{}' execution for context {} (type: {:?})",
        agent_identifier,
        context_id,
        request.execution_type
    );

    let agent = if let Some(ref agent_id_str) = request.agent_id {
        let agent_id: i64 = agent_id_str
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid agent_id '{}': {}", agent_id_str, e))?;
        GetAiAgentByIdWithFeatures::new(agent_id)
            .execute(app_state)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get agent by ID {}: {}", agent_id, e))?
    } else if let Some(ref agent_name) = request.agent_name {
        // Try to get agent from DB first
        match GetAiAgentByNameWithFeatures::new(deployment_id, agent_name.clone())
            .execute(app_state)
            .await
        {
            Ok(agent) => agent,
            Err(_) => {
                // Check if it's a system agent (virtual)
                match agent_name.to_lowercase().as_str() {
                    "teams agent" => {
                        create_virtual_system_agent(
                            deployment_id,
                            "Teams Agent",
                            "Handles Microsoft Teams operations",
                            -1000,
                            app_state,
                        )
                        .await?
                    }
                    "clickup agent" => {
                        create_virtual_system_agent(
                            deployment_id,
                            "ClickUp Agent",
                            "Manages ClickUp tasks and projects",
                            -2000,
                            app_state,
                        )
                        .await?
                    }
                    "whatsapp agent" => {
                        create_virtual_system_agent(
                            deployment_id,
                            "WhatsApp Agent",
                            "Processes WhatsApp messages",
                            -3000,
                            app_state,
                        )
                        .await?
                    }
                    _ => {
                        return Err(anyhow::anyhow!("Agent '{}' not found", agent_name));
                    }
                }
            }
        }
    } else {
        return Err(anyhow::anyhow!(
            "Either agent_id or agent_name must be provided"
        ));
    };

    let execution_request = match request.execution_type {
        AgentExecutionType::NewMessage {
            ref conversation_id,
        } => {
            let conv_id: i64 = conversation_id.parse().map_err(|e| {
                anyhow::anyhow!("Invalid conversation_id '{}': {}", conversation_id, e)
            })?;
            tracing::info!("New message execution with conversation_id: {}", conv_id);

            if let Ok(conversation) = queries::GetConversationByIdQuery::new(conv_id)
                .execute(app_state)
                .await
            {
                let webhook_payload = serde_json::json!({
                    "context_id": context_id,
                    "message_type": "conversation_message",
                    "data": conversation.content,
                    "timestamp": conversation.timestamp,
                });

                let console_id = std::env::var("CONSOLE_DEPLOYMENT_ID")
                    .unwrap_or_else(|_| "0".to_string())
                    .parse()
                    .unwrap_or(0);

                let trigger_command = TriggerWebhookEventCommand::new(
                    console_id,
                    deployment_id.to_string(),
                    "execution_context.message".to_string(),
                    webhook_payload,
                );

                if let Err(e) = trigger_command.execute(app_state).await {
                    tracing::error!("Failed to trigger user message webhook: {}", e);
                }
            }

            ExecutionRequest {
                agent,
                conversation_id: Some(conv_id),
                context_id,
                platform_function_result: None,
            }
        }
        AgentExecutionType::UserInputResponse {
            ref conversation_id,
        } => {
            let conv_id: i64 = conversation_id.parse().map_err(|e| {
                anyhow::anyhow!("Invalid conversation_id '{}': {}", conversation_id, e)
            })?;
            tracing::info!("User input response with conversation_id: {}", conv_id);

            if let Ok(conversation) = queries::GetConversationByIdQuery::new(conv_id)
                .execute(app_state)
                .await
            {
                let webhook_payload = serde_json::json!({
                    "context_id": context_id,
                    "message_type": "user_input_response",
                    "data": conversation.content,
                    "timestamp": conversation.timestamp,
                });

                let console_id = std::env::var("CONSOLE_DEPLOYMENT_ID")
                    .unwrap_or_else(|_| "0".to_string())
                    .parse()
                    .unwrap_or(0);

                let trigger_command = TriggerWebhookEventCommand::new(
                    console_id,
                    deployment_id.to_string(),
                    "execution_context.message".to_string(),
                    webhook_payload,
                );

                if let Err(e) = trigger_command.execute(app_state).await {
                    tracing::error!("Failed to trigger user response webhook: {}", e);
                }
            }

            ExecutionRequest {
                agent,
                conversation_id: Some(conv_id),
                context_id,
                platform_function_result: None,
            }
        }
        AgentExecutionType::PlatformFunctionResult {
            execution_id,
            result,
        } => {
            tracing::info!(
                "Platform function result for execution_id: {}",
                execution_id
            );

            let webhook_payload = serde_json::json!({
                "context_id": context_id,
                "message_type": "platform_function_result",
                "execution_id": execution_id,
                "data": result,
                "timestamp": chrono::Utc::now(),
            });

            let console_id = std::env::var("CONSOLE_DEPLOYMENT_ID")
                .unwrap_or_else(|_| "0".to_string())
                .parse()
                .unwrap_or(0);

            let trigger_command = TriggerWebhookEventCommand::new(
                console_id,
                deployment_id.to_string(),
                "execution_context.platform_function_result".to_string(),
                webhook_payload,
            );

            if let Err(e) = trigger_command.execute(app_state).await {
                tracing::error!("Failed to trigger platform function result webhook: {}", e);
            }

            ExecutionRequest {
                agent,
                conversation_id: None,
                context_id,
                platform_function_result: Some((execution_id, result)),
            }
        }
    };

    AgentHandler::new(app_state.clone())
        .execute_agent_streaming(execution_request)
        .await
        .map_err(|e| anyhow::anyhow!("Agent execution failed: {}", e))?;

    Ok(format!(
        "Agent '{}' execution completed for context {}",
        agent_identifier, context_id
    ))
}
