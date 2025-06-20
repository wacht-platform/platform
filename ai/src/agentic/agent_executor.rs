use super::{AgentContext, ToolCall, ToolResult, XmlParser};
use futures_util::stream::StreamExt;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use shared::error::AppError;
use shared::models::AiAgent;
use shared::queries::{
    GetAiAgentByNameQuery, GetAiKnowledgeBasesByIdsQuery, GetAiToolsByIdsQuery,
    GetAiWorkflowsByIdsQuery, Query,
};
use shared::state::AppState;

pub struct AgentExecutor {
    pub agent: AiAgent,
    pub context: AgentContext,
    pub app_state: AppState,
}

impl AgentExecutor {
    pub async fn new(
        agent_name: &str,
        deployment_id: i64,
        app_state: &AppState,
    ) -> Result<Self, AppError> {
        // Fetch agent by name
        let agent = GetAiAgentByNameQuery::new(deployment_id, agent_name.to_string())
            .execute(app_state)
            .await?;

        // Extract IDs from agent configuration
        let tool_ids = agent
            .configuration
            .get("tool_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let workflow_ids = agent
            .configuration
            .get("workflow_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let knowledge_base_ids = agent
            .configuration
            .get("knowledge_base_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        // Fetch tools, workflows, and knowledge bases
        let tools = if !tool_ids.is_empty() {
            GetAiToolsByIdsQuery::new(deployment_id, tool_ids)
                .execute(app_state)
                .await?
        } else {
            Vec::new()
        };

        let workflows = if !workflow_ids.is_empty() {
            GetAiWorkflowsByIdsQuery::new(deployment_id, workflow_ids)
                .execute(app_state)
                .await?
        } else {
            Vec::new()
        };

        let knowledge_bases = if !knowledge_base_ids.is_empty() {
            GetAiKnowledgeBasesByIdsQuery::new(deployment_id, knowledge_base_ids)
                .execute(app_state)
                .await?
        } else {
            Vec::new()
        };

        let context = AgentContext {
            agent_id: agent.id,
            deployment_id,
            tools,
            workflows,
            knowledge_bases,
        };

        Ok(Self {
            agent,
            context,
            app_state: app_state.clone(),
        })
    }

    pub fn get_system_prompt(&self) -> String {
        let mut all_tools = Vec::new();

        for tool in &self.context.tools {
            let params = match &tool.configuration {
                shared::models::AiToolConfiguration::Api(config) => {
                    let mut params = Vec::new();
                    for header in &config.headers {
                        params.push(format!("    - {} (header): {}", header.name, header.description.as_deref().unwrap_or("No description")));
                    }
                    for param in &config.query_parameters {
                        params.push(format!("    - {} (query): {}", param.name, param.description.as_deref().unwrap_or("No description")));
                    }
                    for param in &config.body_parameters {
                        params.push(format!("    - {} (body): {}", param.name, param.description.as_deref().unwrap_or("No description")));
                    }
                    if params.is_empty() { "    No parameters".to_string() } else { params.join("\n") }
                },
                shared::models::AiToolConfiguration::KnowledgeBase(_) => {
                    "    - query (required): Search query text\n    - max_results (optional): Maximum number of results (default: 10)\n    - similarity_threshold (optional): Minimum similarity score (default: 0.7)\n    - include_metadata (optional): Include document metadata (default: true)".to_string()
                },
                shared::models::AiToolConfiguration::PlatformFunction(config) => {
                    if let Some(schema) = &config.input_schema {
                        schema.iter()
                            .map(|field| format!("    - {} ({}): {}", field.name, field.field_type, field.description.as_deref().unwrap_or("No description")))
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        "    No parameters defined".to_string()
                    }
                },
                shared::models::AiToolConfiguration::PlatformEvent(_) => {
                    "    - event_data (optional): Additional event data".to_string()
                }
            };

            all_tools.push(format!(
                "- tool_{} ({:?}): {}\n  Parameters:\n{}",
                tool.name,
                tool.tool_type,
                tool.description.as_deref().unwrap_or("No description"),
                params
            ));
        }

        for workflow in &self.context.workflows {
            all_tools.push(format!("- workflow_{}: {}\n  Parameters:\n    - input_data (optional): Input data for the workflow",
                workflow.name,
                workflow.description.as_deref().unwrap_or("No description")
            ));
        }

        let tools_list = all_tools.join("\n\n");

        format!(
            r#"You are {}, an AI assistant with access to various tools and workflows.

{}

## Available Tools and Workflows:
{}

## Context Engine:
You have access to a powerful "context_engine" tool that can retrieve any information available to you:

- **context_engine**: Retrieve any available context or information
  Parameters:
    - query (required): What you're looking for
    - max_results (optional): Maximum results (default: 20)

## Tool Usage Format:
<tool_call>
<name>tool_name</name>
<id>unique_id</id>
<arguments>
<parameter_name>parameter_value</parameter_name>
</arguments>
</tool_call>

## Instructions:
1. Use context_engine to find information when needed
2. Use tool_ prefixed names for regular tools
3. Use workflow_ prefixed names for workflows
4. Always provide clear explanations of your actions"#,
            self.agent.name,
            self.agent.description.as_deref().unwrap_or(""),
            if tools_list.is_empty() {
                "None available"
            } else {
                &tools_list
            }
        )
    }

    pub async fn execute_with_streaming<F>(
        &self,
        user_message: &str,
        mut on_chunk: F,
    ) -> Result<(), AppError>
    where
        F: FnMut(&str) + Send,
    {
        // Create LLM for this execution
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.0-flash")
            .max_tokens(8000)
            .temperature(0.7)
            .stream(true)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM: {}", e)))?;

        let system_prompt = self.get_system_prompt();

        let mut conversation = vec![
            ChatMessage::user()
                .content(&format!("{}\n\nUser: {}", system_prompt, user_message))
                .build(),
        ];

        let mut stream = llm
            .chat_stream(&conversation)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to start chat stream: {}", e)))?;

        let mut xml_parser = XmlParser::new();

        while let Some(chunk_result) = stream.next().await {
            match chunk_result {
                Ok(chunk) => {
                    let (text_content, tool_calls) = xml_parser.parse_chunk(&chunk);

                    // Send text content to user
                    if let Some(content) = text_content {
                        on_chunk(&content);
                    }

                    // Process tool calls if any
                    if !tool_calls.is_empty() {
                        for tool_call in &tool_calls {
                            on_chunk(&format!("\n[Executing tool: {}]\n", tool_call.name));

                            // Execute the tool call with conversation history
                            let result = self
                                .execute_tool_call_with_history(tool_call, &conversation)
                                .await;
                            match result {
                                Ok(tool_result) => {
                                    on_chunk(&format!(
                                        "[Tool result: {}]\n",
                                        serde_json::to_string_pretty(&tool_result.result)
                                            .unwrap_or_default()
                                    ));
                                }
                                Err(e) => {
                                    on_chunk(&format!("[Tool error: {}]\n", e));
                                }
                            }
                        }

                        // Continue conversation with tool results
                        // This would require implementing tool result handling in the conversation
                        // For now, we'll just acknowledge the tool execution
                        break;
                    }
                }
                Err(e) => {
                    return Err(AppError::Internal(format!("Stream error: {}", e)));
                }
            }
        }

        Ok(())
    }

    async fn execute_tool_call_with_history(
        &self,
        tool_call: &ToolCall,
        conversation_history: &[ChatMessage],
    ) -> Result<ToolResult, AppError> {
        use super::tool_executor::ToolExecutor;

        let tool_executor = ToolExecutor::new(
            self.context.clone(),
            self.app_state.clone(),
            conversation_history.to_vec(),
        );
        tool_executor.execute_tool_call(tool_call).await
    }
}
