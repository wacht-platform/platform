use commands::{Command, TriggerWebhookEventCommand};
use common::state::AppState;
use serde::{Deserialize, Serialize};



/// Process an agent execution request
/// Handles NewMessage, PlatformFunctionResult, and UserInputResponse
pub async fn process_agent_execution(
    app_state: &AppState,
    request: dto::json::AgentExecutionRequest,
) -> Result<String, anyhow::Error> {
    use agent_engine::{AgentHandler, ExecutionRequest};
    use dto::json::AgentExecutionType;
    use queries::{GetAiAgentByNameWithFeatures, Query};

    tracing::info!(
        "Processing agent '{}' execution for context {} (type: {:?})",
        request.agent_name,
        request.context_id,
        request.execution_type
    );

    // Fetch the agent by name
    let agent = GetAiAgentByNameWithFeatures::new(request.deployment_id, request.agent_name.clone())
        .execute(app_state)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get agent '{}': {}", request.agent_name, e))?;

    // Build ExecutionRequest based on execution type
    let execution_request = match request.execution_type {
        AgentExecutionType::NewMessage { conversation_id } => {
            tracing::info!("New message execution with conversation_id: {}", conversation_id);
            
            if let Ok(conversation) = queries::GetConversationByIdQuery::new(conversation_id).execute(app_state).await {
                let webhook_payload = serde_json::json!({
                    "context_id": request.context_id,
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
                    request.deployment_id.clone(),
                    "execution_context.message".to_string(),
                    webhook_payload,
                );

                if let Err(e) = trigger_command.execute(app_state).await {
                    tracing::error!("Failed to trigger user message webhook: {}", e);
                }
            }

            ExecutionRequest {
                agent,
                conversation_id: Some(conversation_id),
                context_id: request.context_id,
                platform_function_result: None,
            }
        }
        AgentExecutionType::UserInputResponse { conversation_id } => {
            tracing::info!("User input response with conversation_id: {}", conversation_id);

            if let Ok(conversation) = queries::GetConversationByIdQuery::new(conversation_id).execute(app_state).await {
                let webhook_payload = serde_json::json!({
                    "context_id": request.context_id,
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
                    request.deployment_id.clone(),
                    "execution_context.message".to_string(),
                    webhook_payload,
                );

                if let Err(e) = trigger_command.execute(app_state).await {
                     tracing::error!("Failed to trigger user response webhook: {}", e);
                }
            }

            ExecutionRequest {
                agent,
                conversation_id: Some(conversation_id),
                context_id: request.context_id,
                platform_function_result: None,
            }
        }
        AgentExecutionType::PlatformFunctionResult { execution_id, result } => {
            tracing::info!("Platform function result for execution_id: {}", execution_id);
            
            let webhook_payload = serde_json::json!({
                "context_id": request.context_id,
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
                request.deployment_id.clone(),
                "execution_context.platform_function_result".to_string(),
                webhook_payload,
            );

            if let Err(e) = trigger_command.execute(app_state).await {
                    tracing::error!("Failed to trigger platform function result webhook: {}", e);
            }

            ExecutionRequest {
                agent,
                conversation_id: None,
                context_id: request.context_id,
                platform_function_result: Some((execution_id, result)),
            }
        }
    };

    // Execute the agent
    AgentHandler::new(app_state.clone())
        .execute_agent_streaming(execution_request)
        .await
        .map_err(|e| anyhow::anyhow!("Agent execution failed: {}", e))?;

    Ok(format!(
        "Agent '{}' execution completed for context {}",
        request.agent_name, request.context_id
    ))
}

