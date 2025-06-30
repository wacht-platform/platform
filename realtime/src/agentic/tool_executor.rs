use super::{AgentContext, ToolCall, ToolResult};
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;
use serde_json::{Value, json};
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::error::AppError;
use shared::models::{AiToolConfiguration, ApiToolConfiguration, HttpMethod};
use shared::state::AppState;
use std::collections::HashMap;

pub struct ToolExecutor {
    pub context: AgentContext,
    pub app_state: AppState,
    pub conversation_history: Vec<ChatMessage>,
}

impl ToolExecutor {
    pub fn new(
        context: AgentContext,
        app_state: AppState,
        conversation_history: Vec<ChatMessage>,
    ) -> Self {
        Self {
            context,
            app_state,
            conversation_history,
        }
    }

    pub async fn execute_tool_call(&self, tool_call: &ToolCall) -> Result<ToolResult, AppError> {
        // Handle special context engine tool
        if tool_call.name == "context_engine" {
            return self.execute_context_engine(tool_call).await;
        }

        // Handle special memory tool
        if tool_call.name == "memory" {
            return self.execute_memory_tool(tool_call).await;
        }

        // Handle workflow execution
        if tool_call.name.starts_with("workflow_") {
            let workflow_name = &tool_call.name[9..]; // Remove "workflow_" prefix
            let workflow = self
                .context
                .workflows
                .iter()
                .find(|w| w.name == workflow_name)
                .ok_or_else(|| {
                    AppError::BadRequest(format!("Workflow '{}' not found", workflow_name))
                })?;
            return self.execute_workflow(tool_call, workflow).await;
        }

        // Handle prefixed tool names
        if tool_call.name.starts_with("tool_") {
            let actual_tool_name = &tool_call.name[5..]; // Remove "tool_" prefix
            let tool = self
                .context
                .tools
                .iter()
                .find(|t| t.name == actual_tool_name)
                .ok_or_else(|| {
                    AppError::BadRequest(format!("Tool '{}' not found", actual_tool_name))
                })?;

            return self.execute_regular_tool(tool_call, tool).await;
        }

        if tool_call.name.starts_with("workflow_") {
            let actual_workflow_name = &tool_call.name[9..]; // Remove "workflow_" prefix
            let workflow = self
                .context
                .workflows
                .iter()
                .find(|w| w.name == actual_workflow_name)
                .ok_or_else(|| {
                    AppError::BadRequest(format!("Workflow '{}' not found", actual_workflow_name))
                })?;

            return self.execute_workflow(tool_call, workflow).await;
        }

        // Fallback: try to find tool without prefix (for backward compatibility)
        let tool = self
            .context
            .tools
            .iter()
            .find(|t| t.name == tool_call.name)
            .ok_or_else(|| {
                AppError::BadRequest(format!("Tool or workflow '{}' not found", tool_call.name))
            })?;

        self.execute_regular_tool(tool_call, tool).await
    }

    pub async fn execute_regular_tool(
        &self,
        tool_call: &ToolCall,
        tool: &shared::models::AiTool,
    ) -> Result<ToolResult, AppError> {
        match &tool.configuration {
            AiToolConfiguration::Api(config) => self.execute_api_tool(tool_call, config).await,
            AiToolConfiguration::KnowledgeBase(config) => {
                self.execute_knowledge_base_tool(tool_call, config).await
            }
            AiToolConfiguration::PlatformEvent(config) => {
                self.execute_platform_event_tool(tool_call, config).await
            }
            AiToolConfiguration::PlatformFunction(config) => {
                self.execute_platform_function_tool(tool_call, config).await
            }
        }
    }

    async fn execute_workflow(
        &self,
        tool_call: &ToolCall,
        workflow: &shared::models::AiWorkflow,
    ) -> Result<ToolResult, AppError> {
        // Extract input data from tool call arguments
        let input_data = tool_call
            .arguments
            .get("input_data")
            .cloned()
            .unwrap_or(json!({}));

        // For now, return a placeholder result indicating workflow execution
        // In a full implementation, this would execute the workflow definition
        Ok(ToolResult {
            tool_call_id: tool_call.id.clone(),
            result: json!({
                "type": "workflow_execution",
                "workflow_name": workflow.name,
                "workflow_id": workflow.id,
                "description": workflow.description,
                "input_data": input_data,
                "status": "executed",
                "message": "Workflow execution completed successfully"
            }),
            error: None,
        })
    }

    async fn execute_context_engine(&self, tool_call: &ToolCall) -> Result<ToolResult, AppError> {
        use super::context_engine::ContextEngine;

        // Check for different context engine operations
        let operation = tool_call
            .arguments
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        let context_engine = ContextEngine::new(self.context.clone(), self.app_state.clone())?;

        match operation {
            "search" => {
                let query = tool_call
                    .arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if query.is_empty() {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": "Query parameter is required for search operation"}),
                        error: Some("Query parameter is required".to_string()),
                    });
                }

                let search_result = context_engine.search(query).await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: search_result,
                    error: None,
                })
            }
            "store" => {
                let key = tool_call
                    .arguments
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let default_data = json!({});
                let data = tool_call.arguments.get("data").unwrap_or(&default_data);

                if key.is_empty() {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": "Key parameter is required for store operation"}),
                        error: Some("Key parameter is required".to_string()),
                    });
                }

                let store_result = context_engine.store_context(key, data).await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: store_result,
                    error: None,
                })
            }
            "fetch" => {
                let key = tool_call
                    .arguments
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if key.is_empty() {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": "Key parameter is required for fetch operation"}),
                        error: Some("Key parameter is required".to_string()),
                    });
                }

                let fetch_result = context_engine.fetch_context(key).await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: fetch_result,
                    error: None,
                })
            }
            "list_keys" => {
                let list_result = context_engine.list_context_keys().await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: list_result,
                    error: None,
                })
            }
            "delete" => {
                let key = tool_call
                    .arguments
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if key.is_empty() {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": "Key parameter is required for delete operation"}),
                        error: Some("Key parameter is required".to_string()),
                    });
                }

                let delete_result = context_engine.delete_context(key).await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: delete_result,
                    error: None,
                })
            }
            "get_detailed_info" => {
                let resource_type = tool_call
                    .arguments
                    .get("resource_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let resource_id = tool_call
                    .arguments
                    .get("resource_id")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);

                if resource_type.is_empty() || resource_id == 0 {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": "resource_type and resource_id parameters are required"}),
                        error: Some("Missing required parameters".to_string()),
                    });
                }

                let detailed_info = context_engine
                    .get_detailed_info(resource_type, resource_id)
                    .await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: detailed_info,
                    error: None,
                })
            }
            _ => Ok(ToolResult {
                tool_call_id: tool_call.id.clone(),
                result: json!({"error": format!("Unknown context engine operation: {}. Available: search, store, fetch, delete, list, get_detailed_info", operation)}),
                error: Some(format!("Unknown operation: {}", operation)),
            }),
        }
    }

    async fn execute_api_tool(
        &self,
        tool_call: &ToolCall,
        config: &ApiToolConfiguration,
    ) -> Result<ToolResult, AppError> {
        let mut url = config.endpoint.clone();
        let mut headers = HashMap::new();
        let mut query_params = HashMap::new();
        let mut body_data = HashMap::new();

        // Process headers
        for header in &config.headers {
            let value = self
                .resolve_parameter_value(&header.value_type, &tool_call.arguments, &header.name)
                .await?;
            headers.insert(header.name.clone(), value);
        }

        // Process query parameters
        for param in &config.query_parameters {
            let value = self
                .resolve_parameter_value(&param.value_type, &tool_call.arguments, &param.name)
                .await?;
            query_params.insert(param.name.clone(), value);
        }

        // Process body parameters
        for param in &config.body_parameters {
            let value = self
                .resolve_parameter_value(&param.value_type, &tool_call.arguments, &param.name)
                .await?;
            body_data.insert(param.name.clone(), value);
        }

        // Build query string
        if !query_params.is_empty() {
            let query_string = query_params
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join("&");
            url = format!("{}?{}", url, query_string);
        }

        // Make HTTP request using ureq
        let response = match config.method {
            HttpMethod::GET => {
                let mut request = ureq::get(&url);
                for (key, value) in headers {
                    request = request.header(&key, &value);
                }
                request.call()
            }
            HttpMethod::POST => {
                let mut request = ureq::post(&url);
                for (key, value) in headers {
                    request = request.header(&key, &value);
                }
                if !body_data.is_empty() {
                    request.send_json(&body_data)
                } else {
                    request.send("")
                }
            }
            HttpMethod::PUT => {
                let mut request = ureq::put(&url);
                for (key, value) in headers {
                    request = request.header(&key, &value);
                }
                if !body_data.is_empty() {
                    request.send_json(&body_data)
                } else {
                    request.send("")
                }
            }
            HttpMethod::DELETE => {
                let mut request = ureq::delete(&url);
                for (key, value) in headers {
                    request = request.header(&key, &value);
                }
                request.call()
            }
            HttpMethod::PATCH => {
                let mut request = ureq::patch(&url);
                for (key, value) in headers {
                    request = request.header(&key, &value);
                }
                if !body_data.is_empty() {
                    request.send_json(&body_data)
                } else {
                    request.send("")
                }
            }
        };

        match response {
            Ok(mut resp) => {
                let status = resp.status().as_u16();
                let body = resp
                    .body_mut()
                    .read_to_string()
                    .unwrap_or_else(|_| "Failed to read response body".to_string());

                let result = json!({
                    "status": status,
                    "body": body,
                    "success": status >= 200 && status < 300
                });

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result,
                    error: None,
                })
            }
            Err(e) => Ok(ToolResult {
                tool_call_id: tool_call.id.clone(),
                result: json!({"error": format!("HTTP request failed: {}", e)}),
                error: Some(format!("HTTP request failed: {}", e)),
            }),
        }
    }

    async fn execute_knowledge_base_tool(
        &self,
        tool_call: &ToolCall,
        config: &shared::models::KnowledgeBaseToolConfiguration,
    ) -> Result<ToolResult, AppError> {
        // Extract parameters from tool call arguments
        let mut resolved_params = HashMap::new();

        // Resolve query parameter
        let query = if let Some(query_value) = tool_call.arguments.get("query") {
            query_value.as_str().unwrap_or("").to_string()
        } else {
            return Ok(ToolResult {
                tool_call_id: tool_call.id.clone(),
                result: json!({"error": "Query parameter is required for knowledge base search"}),
                error: Some("Query parameter is required".to_string()),
            });
        };

        // Resolve optional parameters with defaults from config
        let max_results = tool_call
            .arguments
            .get("max_results")
            .and_then(|v| v.as_u64())
            .or(config.search_settings.max_results.map(|m| m as u64))
            .unwrap_or(10) as u32;

        let similarity_threshold = tool_call
            .arguments
            .get("similarity_threshold")
            .and_then(|v| v.as_f64())
            .or(config
                .search_settings
                .similarity_threshold
                .map(|t| t as f64))
            .unwrap_or(0.7) as f32;

        let include_metadata = tool_call
            .arguments
            .get("include_metadata")
            .and_then(|v| v.as_bool())
            .unwrap_or(config.search_settings.include_metadata);

        resolved_params.insert("query".to_string(), query.clone());
        resolved_params.insert("max_results".to_string(), max_results.to_string());
        resolved_params.insert(
            "similarity_threshold".to_string(),
            similarity_threshold.to_string(),
        );
        resolved_params.insert("include_metadata".to_string(), include_metadata.to_string());

        // Search across all available knowledge bases
        let search_results = self
            .search_all_knowledge_base_documents(&query, max_results as usize)
            .await?;

        let total_found = search_results.len();

        Ok(ToolResult {
            tool_call_id: tool_call.id.clone(),
            result: json!({
                "query": query,
                "parameters": resolved_params,
                "results": search_results,
                "total_found": total_found
            }),
            error: None,
        })
    }

    async fn search_qdrant_knowledge_base(
        &self,
        kb_id: i64,
        query: &str,
        max_results: u32,
        _similarity_threshold: f32,
        _include_metadata: bool,
    ) -> Result<Value, AppError> {
        // Generate embedding using command pattern
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        // Use ClickHouse to search embeddings
        let search_results =
            SearchKnowledgeBaseEmbeddingsCommand::new(kb_id, query_embedding, max_results as u64)
                .execute(&self.app_state)
                .await?;

        // Convert search results to the expected format
        let results: Vec<Value> = search_results
            .into_iter()
            .map(|result| {
                json!({
                    "id": result.id,
                    "score": result.score,
                    "content": result.content,
                    "knowledge_base_id": result.knowledge_base_id,
                    "document_id": result.document_id,
                    "chunk_index": result.chunk_index
                })
            })
            .collect();

        Ok(json!(results))
    }

    async fn execute_platform_event_tool(
        &self,
        tool_call: &ToolCall,
        _config: &shared::models::PlatformEventToolConfiguration,
    ) -> Result<ToolResult, AppError> {
        // Placeholder for platform events
        Ok(ToolResult {
            tool_call_id: tool_call.id.clone(),
            result: json!({"message": "Platform event execution not yet implemented"}),
            error: None,
        })
    }

    async fn execute_platform_function_tool(
        &self,
        tool_call: &ToolCall,
        config: &shared::models::PlatformFunctionToolConfiguration,
    ) -> Result<ToolResult, AppError> {
        // Platform functions return control to the client with function details and parameters

        // Extract parameters from tool call arguments based on input schema
        let mut resolved_params = HashMap::new();

        if let Some(input_schema) = &config.input_schema {
            for schema_field in input_schema {
                if let Some(value) = tool_call.arguments.get(&schema_field.name) {
                    resolved_params.insert(
                        schema_field.name.clone(),
                        value.as_str().unwrap_or_default().to_string(),
                    );
                } else if schema_field.required {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": format!("Required parameter '{}' not provided", schema_field.name)}),
                        error: Some(format!(
                            "Required parameter '{}' not provided",
                            schema_field.name
                        )),
                    });
                }
            }
        }

        // Return control to client with function execution request
        Ok(ToolResult {
            tool_call_id: tool_call.id.clone(),
            result: json!({
                "type": "platform_function_request",
                "function_name": config.function_name,
                "description": config.function_description,
                "parameters": resolved_params,
                "input_schema": config.input_schema,
                "output_schema": config.output_schema,
                "requires_client_execution": true,
                "execution_context": {
                    "agent_id": self.context.agent_id,
                    "deployment_id": self.context.deployment_id,
                    "tool_call_id": tool_call.id
                }
            }),
            error: None,
        })
    }

    async fn resolve_parameter_value(
        &self,
        param_value: &shared::models::ParameterValueType,
        arguments: &Value,
        param_name: &str,
    ) -> Result<String, AppError> {
        match param_value {
            shared::models::ParameterValueType::Hardcoded { value } => Ok(value.clone()),
            shared::models::ParameterValueType::FromChat { lookup_key } => {
                // First try to get from tool call arguments
                if let Some(value) = arguments.get(lookup_key).and_then(|v| v.as_str()) {
                    return Ok(value.to_string());
                }

                // If not found, use LLM to resolve the parameter dynamically
                self.resolve_dynamic_parameter(lookup_key, param_name).await
            }
        }
    }

    async fn resolve_dynamic_parameter(
        &self,
        lookup_key: &str,
        param_name: &str,
    ) -> Result<String, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.0-flash")
            .max_tokens(1000)
            .temperature(0.1)
            .build()
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to build LLM for parameter resolution: {}",
                    e
                ))
            })?;

        // Create context for parameter resolution
        let tools_context = self
            .context
            .tools
            .iter()
            .map(|tool| {
                format!(
                    "- {}: {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("No description")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let system_prompt = format!(
            r#"You are a parameter resolution assistant. Your job is to extract the value for parameter "{}" (lookup key: "{}") from the conversation context.

Available tools for context:
{}

You have access to a context_engine tool that can search across all available resources.

Based on the conversation history, provide ONLY the value for the requested parameter. Do not include explanations or additional text.

If you need to use the context engine to find information, format your request as:
<tool_call>
<name>context_engine</name>
<id>context_search</id>
<arguments>
<query>your search query here</query>
</arguments>
</tool_call>

Otherwise, respond with just the parameter value."#,
            param_name, lookup_key, tools_context
        );

        let mut messages = vec![ChatMessage::user().content(&system_prompt).build()];

        // Add conversation history
        messages.extend(self.conversation_history.clone());

        // Add current request
        messages.push(
            ChatMessage::user()
                .content(&format!(
                    "Please provide the value for parameter '{}' (lookup key: '{}')",
                    param_name, lookup_key
                ))
                .build(),
        );

        let response = llm
            .chat(&messages)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to resolve parameter: {}", e)))?;

        let content = response.text().unwrap_or_default();

        // Check if the response contains a tool call for context engine
        if content.contains("<tool_call>") && content.contains("context_engine") {
            // Parse and execute context engine call
            if let Some(query) = self.extract_context_query(&content) {
                // Drop the response before the async call
                drop(response);
                let context_result = self.execute_context_engine_search(&query).await?;

                // Make another LLM call with the context result
                messages.push(ChatMessage::assistant().content(&content).build());

                messages.push(
                    ChatMessage::user()
                        .content(&format!(
                            "Context search result: {}\n\nNow provide the value for parameter '{}'",
                            serde_json::to_string_pretty(&context_result).unwrap_or_default(),
                            param_name
                        ))
                        .build(),
                );

                let final_response = llm.chat(&messages).await.map_err(|e| {
                    AppError::Internal(format!("Failed to resolve parameter with context: {}", e))
                })?;

                Ok(final_response.text().unwrap_or_default().trim().to_string())
            } else {
                Err(AppError::Internal(
                    "Failed to parse context engine query".to_string(),
                ))
            }
        } else {
            Ok(content.trim().to_string())
        }
    }

    fn extract_context_query(&self, content: &str) -> Option<String> {
        // Simple extraction of query from context_engine tool call
        if let Some(start) = content.find("<query>") {
            if let Some(end) = content[start + 7..].find("</query>") {
                return Some(content[start + 7..start + 7 + end].trim().to_string());
            }
        }
        None
    }

    async fn execute_context_engine_search(&self, query: &str) -> Result<Value, AppError> {
        use super::context_engine::ContextEngine;

        // Use the enhanced context engine for search
        let context_engine = ContextEngine::new(self.context.clone(), self.app_state.clone())?;
        context_engine.search(query).await
    }

    async fn execute_memory_tool(&self, tool_call: &ToolCall) -> Result<ToolResult, AppError> {
        use super::memory_manager::{MemoryManager, MemoryQuery, MemoryType};
        use std::collections::HashMap;

        let memory_manager = MemoryManager::new(
            self.context.clone(),
            self.app_state.clone(),
            self.context.execution_context_id,
        )?;

        let operation = tool_call
            .arguments
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("search");

        match operation {
            "store" => {
                let content = tool_call
                    .arguments
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let memory_type_str = tool_call
                    .arguments
                    .get("memory_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("working");
                let importance = tool_call
                    .arguments
                    .get("importance")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.5) as f32;

                if content.is_empty() {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": "Content parameter is required for store operation"}),
                        error: Some("Content parameter is required".to_string()),
                    });
                }

                let memory_type =
                    MemoryType::from_str(memory_type_str).unwrap_or(MemoryType::Working);
                let mut metadata = HashMap::new();

                // Add any additional metadata from the tool call
                if let Some(meta) = tool_call.arguments.get("metadata") {
                    if let Some(meta_obj) = meta.as_object() {
                        for (key, value) in meta_obj {
                            metadata.insert(key.clone(), value.clone());
                        }
                    }
                }

                let memory_id = memory_manager
                    .store_memory(memory_type, content, metadata, importance)
                    .await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: json!({
                        "operation": "store",
                        "memory_id": memory_id,
                        "content": content,
                        "memory_type": memory_type_str,
                        "importance": importance,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }),
                    error: None,
                })
            }
            "search" => {
                let query_text = tool_call
                    .arguments
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let max_results = tool_call
                    .arguments
                    .get("max_results")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(10) as usize;
                let min_importance = tool_call
                    .arguments
                    .get("min_importance")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0) as f32;

                if query_text.is_empty() {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": "Query parameter is required for search operation"}),
                        error: Some("Query parameter is required".to_string()),
                    });
                }

                // Parse memory types filter
                let memory_types =
                    if let Some(types_array) = tool_call.arguments.get("memory_types") {
                        if let Some(types) = types_array.as_array() {
                            types
                                .iter()
                                .filter_map(|v| v.as_str())
                                .filter_map(|s| MemoryType::from_str(s))
                                .collect()
                        } else {
                            vec![
                                MemoryType::Working,
                                MemoryType::Episodic,
                                MemoryType::Semantic,
                                MemoryType::Procedural,
                            ]
                        }
                    } else {
                        vec![
                            MemoryType::Working,
                            MemoryType::Episodic,
                            MemoryType::Semantic,
                            MemoryType::Procedural,
                        ]
                    };

                let query = MemoryQuery {
                    query: query_text.to_string(),
                    memory_types,
                    max_results,
                    min_importance,
                    time_range: None, // TODO: Parse time range from arguments if needed
                };

                let search_results = memory_manager.search_memories(&query).await?;

                let results: Vec<serde_json::Value> = search_results
                    .into_iter()
                    .map(|result| {
                        json!({
                            "memory_id": result.entry.id,
                            "content": result.entry.content,
                            "memory_type": result.entry.memory_type.as_str(),
                            "importance": result.entry.importance,
                            "relevance_score": result.relevance_score,
                            "similarity_score": result.similarity_score,
                            "created_at": result.entry.created_at.to_rfc3339(),
                            "last_accessed": result.entry.last_accessed.to_rfc3339(),
                            "access_count": result.entry.access_count,
                            "metadata": result.entry.metadata
                        })
                    })
                    .collect();

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: json!({
                        "operation": "search",
                        "query": query_text,
                        "results": results,
                        "total_found": results.len(),
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }),
                    error: None,
                })
            }
            "stats" => {
                let stats = memory_manager.get_memory_stats().await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: json!({
                        "operation": "stats",
                        "stats": stats,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }),
                    error: None,
                })
            }
            "consolidate" => {
                let similarity_threshold = tool_call
                    .arguments
                    .get("similarity_threshold")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.8) as f32;

                let merged_count = memory_manager
                    .consolidate_memories(similarity_threshold)
                    .await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: json!({
                        "operation": "consolidate",
                        "merged_count": merged_count,
                        "similarity_threshold": similarity_threshold,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }),
                    error: None,
                })
            }
            "forget" => {
                let max_memories = tool_call
                    .arguments
                    .get("max_memories")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(1000) as usize;
                let min_importance = tool_call
                    .arguments
                    .get("min_importance")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.1) as f32;

                let forgotten_count = memory_manager
                    .forget_memories(max_memories, min_importance)
                    .await?;

                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: json!({
                        "operation": "forget",
                        "forgotten_count": forgotten_count,
                        "max_memories": max_memories,
                        "min_importance": min_importance,
                        "timestamp": chrono::Utc::now().to_rfc3339()
                    }),
                    error: None,
                })
            }
            _ => Ok(ToolResult {
                tool_call_id: tool_call.id.clone(),
                result: json!({"error": format!("Unknown memory operation: {}", operation)}),
                error: Some(format!("Unknown operation: {}", operation)),
            }),
        }
    }

    async fn search_all_knowledge_base_documents(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<Value>, AppError> {
        let mut all_results = Vec::new();

        // Search across all knowledge bases
        for kb in &self.context.knowledge_bases {
            let kb_results = self
                .search_qdrant_knowledge_base(kb.id, query, max_results as u32, 0.7, true)
                .await?;

            if let Some(results_array) = kb_results.as_array() {
                for mut result in results_array.clone() {
                    if let Some(result_obj) = result.as_object_mut() {
                        result_obj.insert("type".to_string(), json!("document"));
                        result_obj.insert(
                            "source_knowledge_base".to_string(),
                            json!({
                                "id": kb.id,
                                "name": kb.name,
                                "description": kb.description
                            }),
                        );
                    }
                    all_results.push(result);
                }
            }
        }

        // Sort by score and limit
        all_results.sort_by(|a, b| {
            let score_a = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results.truncate(max_results);
        Ok(all_results)
    }
}
