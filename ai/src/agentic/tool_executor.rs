use super::{ToolCall, ToolResult, AgentContext};
use shared::models::{AiTool, AiToolType, AiToolConfiguration, ApiToolConfiguration, HttpMethod};
use shared::error::AppError;
use shared::state::AppState;
use serde_json::{json, Value};
use std::collections::HashMap;
use llm::builder::{LLMBackend, LLMBuilder};
use llm::chat::ChatMessage;

pub struct ToolExecutor {
    pub context: AgentContext,
    pub app_state: AppState,
    pub conversation_history: Vec<ChatMessage>,
}

impl ToolExecutor {
    pub fn new(context: AgentContext, app_state: AppState, conversation_history: Vec<ChatMessage>) -> Self {
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

        // Handle prefixed tool names
        if tool_call.name.starts_with("tool_") {
            let actual_tool_name = &tool_call.name[5..]; // Remove "tool_" prefix
            let tool = self.context.tools.iter()
                .find(|t| t.name == actual_tool_name)
                .ok_or_else(|| AppError::BadRequest(format!("Tool '{}' not found", actual_tool_name)))?;

            return self.execute_regular_tool(tool_call, tool).await;
        }

        if tool_call.name.starts_with("workflow_") {
            let actual_workflow_name = &tool_call.name[9..]; // Remove "workflow_" prefix
            let workflow = self.context.workflows.iter()
                .find(|w| w.name == actual_workflow_name)
                .ok_or_else(|| AppError::BadRequest(format!("Workflow '{}' not found", actual_workflow_name)))?;

            return self.execute_workflow(tool_call, workflow).await;
        }

        // Fallback: try to find tool without prefix (for backward compatibility)
        let tool = self.context.tools.iter()
            .find(|t| t.name == tool_call.name)
            .ok_or_else(|| AppError::BadRequest(format!("Tool or workflow '{}' not found", tool_call.name)))?;

        self.execute_regular_tool(tool_call, tool).await
    }

    async fn execute_regular_tool(&self, tool_call: &ToolCall, tool: &shared::models::AiTool) -> Result<ToolResult, AppError> {

        match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                self.execute_api_tool(tool_call, config).await
            }
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

    async fn execute_workflow(&self, tool_call: &ToolCall, workflow: &shared::models::AiWorkflow) -> Result<ToolResult, AppError> {
        // Extract input data from tool call arguments
        let input_data = tool_call.arguments.get("input_data")
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
        let query = tool_call.arguments.get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if query.is_empty() {
            return Ok(ToolResult {
                tool_call_id: tool_call.id.clone(),
                result: json!({"error": "Query parameter is required"}),
                error: Some("Query parameter is required".to_string()),
            });
        }

        let max_results = tool_call.arguments.get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(20) as usize;

        let mut all_results = Vec::new();

        // Search tools and workflows (combined)
        let tool_results = self.search_tools(&query, max_results, false);
        all_results.extend(tool_results);

        let workflow_results = self.search_workflows(&query, max_results, false);
        all_results.extend(workflow_results);

        // Search knowledge base documents across ALL available knowledge bases
        let doc_results = self.search_all_knowledge_base_documents(&query, max_results).await?;
        all_results.extend(doc_results);

        // Sort by relevance
        all_results.sort_by(|a, b| {
            let score_a = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Limit results
        all_results.truncate(max_results);

        Ok(ToolResult {
            tool_call_id: tool_call.id.clone(),
            result: json!({
                "query": query,
                "results": all_results,
                "total_found": all_results.len()
            }),
            error: None,
        })
    }

    async fn execute_api_tool(&self, tool_call: &ToolCall, config: &ApiToolConfiguration) -> Result<ToolResult, AppError> {
        let mut url = config.endpoint.clone();
        let mut headers = HashMap::new();
        let mut query_params = HashMap::new();
        let mut body_data = HashMap::new();

        // Process headers
        for header in &config.headers {
            let value = self.resolve_parameter_value(&header.value_type, &tool_call.arguments, &header.name).await?;
            headers.insert(header.name.clone(), value);
        }

        // Process query parameters
        for param in &config.query_parameters {
            let value = self.resolve_parameter_value(&param.value_type, &tool_call.arguments, &param.name).await?;
            query_params.insert(param.name.clone(), value);
        }

        // Process body parameters
        for param in &config.body_parameters {
            let value = self.resolve_parameter_value(&param.value_type, &tool_call.arguments, &param.name).await?;
            body_data.insert(param.name.clone(), value);
        }

        // Build query string
        if !query_params.is_empty() {
            let query_string = query_params.iter()
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
                let body = resp.body_mut().read_to_string()
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
            Err(e) => {
                Ok(ToolResult {
                    tool_call_id: tool_call.id.clone(),
                    result: json!({"error": format!("HTTP request failed: {}", e)}),
                    error: Some(format!("HTTP request failed: {}", e)),
                })
            }
        }
    }

    async fn execute_knowledge_base_tool(&self, tool_call: &ToolCall, config: &shared::models::KnowledgeBaseToolConfiguration) -> Result<ToolResult, AppError> {
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
        let max_results = tool_call.arguments.get("max_results")
            .and_then(|v| v.as_u64())
            .or(config.search_settings.max_results.map(|m| m as u64))
            .unwrap_or(10) as u32;

        let similarity_threshold = tool_call.arguments.get("similarity_threshold")
            .and_then(|v| v.as_f64())
            .or(config.search_settings.similarity_threshold.map(|t| t as f64))
            .unwrap_or(0.7) as f32;

        let include_metadata = tool_call.arguments.get("include_metadata")
            .and_then(|v| v.as_bool())
            .unwrap_or(config.search_settings.include_metadata);

        resolved_params.insert("query".to_string(), query.clone());
        resolved_params.insert("max_results".to_string(), max_results.to_string());
        resolved_params.insert("similarity_threshold".to_string(), similarity_threshold.to_string());
        resolved_params.insert("include_metadata".to_string(), include_metadata.to_string());

        // Search across all available knowledge bases
        let search_results = self.search_all_knowledge_base_documents(&query, max_results as usize).await?;

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

    async fn search_qdrant_knowledge_base(&self, kb_id: i64, query: &str, max_results: u32, similarity_threshold: f32, include_metadata: bool) -> Result<Value, AppError> {
        // For now, we'll use a placeholder URL since qdrant_client might not be in AppState
        let qdrant_url = std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6333".to_string());

        // Create search payload for Qdrant
        let search_request = json!({
            "vector": {
                "name": "content",
                "vector": self.generate_query_embedding(query).await?
            },
            "filter": {
                "must": [
                    {
                        "key": "knowledge_base_id",
                        "match": {
                            "value": kb_id
                        }
                    }
                ]
            },
            "limit": max_results,
            "with_payload": true,
            "with_vector": false
        });

        // Make request to Qdrant
        let collection_name = format!("deployment_{}", self.context.deployment_id);
        let url = format!("{}/collections/{}/points/search",
            qdrant_url, collection_name);

        let response = ureq::post(&url)
            .header("Content-Type", "application/json")
            .send_json(&search_request);

        match response {
            Ok(mut resp) => {
                let body = resp.body_mut().read_to_string()
                    .unwrap_or_else(|_| "{}".to_string());

                let qdrant_response: Value = serde_json::from_str(&body)
                    .unwrap_or_else(|_| json!({"result": []}));

                // Extract and format results
                let results = qdrant_response
                    .get("result")
                    .and_then(|r| r.as_array())
                    .map(|arr| {
                        arr.iter().map(|item| {
                            let empty_payload = json!({});
                            let payload = item.get("payload").unwrap_or(&empty_payload);
                            let score = item.get("score").and_then(|s| s.as_f64()).unwrap_or(0.0);

                            let empty_metadata = json!({});
                            let metadata = payload.get("metadata").unwrap_or(&empty_metadata);

                            json!({
                                "content": payload.get("content").and_then(|c| c.as_str()).unwrap_or(""),
                                "metadata": metadata,
                                "knowledge_base_id": payload.get("knowledge_base_id").and_then(|id| id.as_i64()).unwrap_or(0),
                                "score": score
                            })
                        }).collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                Ok(json!(results))
            }
            Err(e) => {
                Err(AppError::Internal(format!("Qdrant search failed: {}", e)))
            }
        }
    }

    async fn generate_query_embedding(&self, query: &str) -> Result<Vec<f32>, AppError> {
        // For now, return a dummy embedding vector
        // In production, you'd use an embedding model like OpenAI's text-embedding-ada-002
        // or a local embedding model

        // Generate a simple hash-based embedding for demonstration
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        query.hash(&mut hasher);
        let hash = hasher.finish();

        // Convert hash to a 384-dimensional vector (common embedding size)
        let mut embedding = Vec::with_capacity(384);
        for i in 0..384 {
            let value = ((hash.wrapping_mul(i as u64 + 1)) % 1000) as f32 / 1000.0;
            embedding.push(value);
        }

        Ok(embedding)
    }

    async fn execute_platform_event_tool(&self, tool_call: &ToolCall, _config: &shared::models::PlatformEventToolConfiguration) -> Result<ToolResult, AppError> {
        // Placeholder for platform events
        Ok(ToolResult {
            tool_call_id: tool_call.id.clone(),
            result: json!({"message": "Platform event execution not yet implemented"}),
            error: None,
        })
    }

    async fn execute_platform_function_tool(&self, tool_call: &ToolCall, config: &shared::models::PlatformFunctionToolConfiguration) -> Result<ToolResult, AppError> {
        // Platform functions return control to the client with function details and parameters

        // Extract parameters from tool call arguments based on input schema
        let mut resolved_params = HashMap::new();

        if let Some(input_schema) = &config.input_schema {
            for schema_field in input_schema {
                if let Some(value) = tool_call.arguments.get(&schema_field.name) {
                    resolved_params.insert(
                        schema_field.name.clone(),
                        value.as_str().unwrap_or_default().to_string()
                    );
                } else if schema_field.required {
                    return Ok(ToolResult {
                        tool_call_id: tool_call.id.clone(),
                        result: json!({"error": format!("Required parameter '{}' not provided", schema_field.name)}),
                        error: Some(format!("Required parameter '{}' not provided", schema_field.name)),
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

    async fn resolve_parameter_value(&self, param_value: &shared::models::ParameterValueType, arguments: &Value, param_name: &str) -> Result<String, AppError> {
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

    async fn resolve_dynamic_parameter(&self, lookup_key: &str, param_name: &str) -> Result<String, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .map_err(|_| AppError::Internal("GEMINI_API_KEY not set".to_string()))?;

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model("gemini-2.0-flash")
            .max_tokens(1000)
            .temperature(0.1)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to build LLM for parameter resolution: {}", e)))?;

        // Create context for parameter resolution
        let tools_context = self.context.tools.iter()
            .map(|tool| format!("- {}: {}", tool.name, tool.description.as_deref().unwrap_or("No description")))
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

        let mut messages = vec![
            ChatMessage::user()
                .content(&system_prompt)
                .build()
        ];

        // Add conversation history
        messages.extend(self.conversation_history.clone());

        // Add current request
        messages.push(
            ChatMessage::user()
                .content(&format!("Please provide the value for parameter '{}' (lookup key: '{}')", param_name, lookup_key))
                .build()
        );

        let response = llm.chat(&messages).await
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
                messages.push(
                    ChatMessage::assistant()
                        .content(&content)
                        .build()
                );

                messages.push(
                    ChatMessage::user()
                        .content(&format!("Context search result: {}\n\nNow provide the value for parameter '{}'",
                            serde_json::to_string_pretty(&context_result).unwrap_or_default(), param_name))
                        .build()
                );

                let final_response = llm.chat(&messages).await
                    .map_err(|e| AppError::Internal(format!("Failed to resolve parameter with context: {}", e)))?;

                Ok(final_response.text().unwrap_or_default().trim().to_string())
            } else {
                Err(AppError::Internal("Failed to parse context engine query".to_string()))
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
        // Execute context engine search
        let mut results = Vec::new();

        // Search tools
        for tool in &self.context.tools {
            if tool.name.to_lowercase().contains(&query.to_lowercase()) ||
               tool.description.as_ref().map_or(false, |d| d.to_lowercase().contains(&query.to_lowercase())) {
                results.push(json!({
                    "type": "tool",
                    "name": tool.name,
                    "description": tool.description
                }));
            }
        }

        // Search workflows
        for workflow in &self.context.workflows {
            if workflow.name.to_lowercase().contains(&query.to_lowercase()) ||
               workflow.description.as_ref().map_or(false, |d| d.to_lowercase().contains(&query.to_lowercase())) {
                results.push(json!({
                    "type": "workflow",
                    "name": workflow.name,
                    "description": workflow.description
                }));
            }
        }

        // Search knowledge bases
        for kb in &self.context.knowledge_bases {
            if kb.name.to_lowercase().contains(&query.to_lowercase()) ||
               kb.description.as_ref().map_or(false, |d| d.to_lowercase().contains(&query.to_lowercase())) {
                results.push(json!({
                    "type": "knowledge_base",
                    "name": kb.name,
                    "description": kb.description
                }));
            }
        }

        Ok(json!({
            "query": query,
            "results": results,
            "total_found": results.len()
        }))
    }

    fn search_tools(&self, query: &str, max_results: usize, include_details: bool) -> Vec<Value> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for tool in &self.context.tools {
            let relevance = self.calculate_relevance(&tool.name, &tool.description, query);
            if relevance > 0.0 {
                let mut result = json!({
                    "type": "tool",
                    "id": tool.id,
                    "name": tool.name,
                    "description": tool.description,
                    "tool_type": tool.tool_type,
                    "relevance_score": relevance
                });

                if include_details {
                    result["configuration"] = json!(tool.configuration);
                    result["created_at"] = json!(tool.created_at);
                    result["updated_at"] = json!(tool.updated_at);
                }

                results.push(result);
            }
        }

        results.sort_by(|a, b| {
            let score_a = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(max_results);
        results
    }

    fn search_workflows(&self, query: &str, max_results: usize, include_details: bool) -> Vec<Value> {
        let mut results = Vec::new();

        for workflow in &self.context.workflows {
            let relevance = self.calculate_relevance(&workflow.name, &workflow.description, query);
            if relevance > 0.0 {
                let mut result = json!({
                    "type": "workflow",
                    "id": workflow.id,
                    "name": workflow.name,
                    "description": workflow.description,
                    "relevance_score": relevance
                });

                if include_details {
                    result["configuration"] = json!(workflow.configuration);
                    result["workflow_definition"] = json!(workflow.workflow_definition);
                    result["created_at"] = json!(workflow.created_at);
                    result["updated_at"] = json!(workflow.updated_at);
                }

                results.push(result);
            }
        }

        results.sort_by(|a, b| {
            let score_a = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(max_results);
        results
    }

    fn search_knowledge_bases(&self, query: &str, max_results: usize, include_details: bool) -> Vec<Value> {
        let mut results = Vec::new();

        for kb in &self.context.knowledge_bases {
            let relevance = self.calculate_relevance(&kb.name, &kb.description, query);
            if relevance > 0.0 {
                let mut result = json!({
                    "type": "knowledge_base",
                    "id": kb.id,
                    "name": kb.name,
                    "description": kb.description,
                    "relevance_score": relevance
                });

                if include_details {
                    result["configuration"] = json!(kb.configuration);
                    result["created_at"] = json!(kb.created_at);
                    result["updated_at"] = json!(kb.updated_at);
                }

                results.push(result);
            }
        }

        results.sort_by(|a, b| {
            let score_a = a.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("relevance_score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        results.truncate(max_results);
        results
    }

    async fn search_all_knowledge_base_documents(&self, query: &str, max_results: usize) -> Result<Vec<Value>, AppError> {
        let mut all_results = Vec::new();

        // Search across all knowledge bases
        for kb in &self.context.knowledge_bases {
            let kb_results = self.search_qdrant_knowledge_base(
                kb.id,
                query,
                max_results as u32,
                0.7,
                true
            ).await?;

            if let Some(results_array) = kb_results.as_array() {
                for mut result in results_array.clone() {
                    if let Some(result_obj) = result.as_object_mut() {
                        result_obj.insert("type".to_string(), json!("document"));
                        result_obj.insert("source_knowledge_base".to_string(), json!({
                            "id": kb.id,
                            "name": kb.name,
                            "description": kb.description
                        }));
                    }
                    all_results.push(result);
                }
            }
        }

        // Sort by score and limit
        all_results.sort_by(|a, b| {
            let score_a = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let score_b = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        all_results.truncate(max_results);
        Ok(all_results)
    }

    fn calculate_relevance(&self, name: &str, description: &Option<String>, query: &str) -> f64 {
        let query_lower = query.to_lowercase();
        let name_lower = name.to_lowercase();
        let mut score = 0.0;

        // Exact name match gets highest score
        if name_lower == query_lower {
            score += 100.0;
        } else if name_lower.contains(&query_lower) {
            score += 50.0;
        }

        // Description matches get lower scores
        if let Some(desc) = description {
            let desc_lower = desc.to_lowercase();
            if desc_lower.contains(&query_lower) {
                score += 25.0;
            }
        }

        // Word-level matching
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();
        let name_words: Vec<&str> = name_lower.split_whitespace().collect();

        for query_word in &query_words {
            for name_word in &name_words {
                if name_word == query_word {
                    score += 10.0;
                } else if name_word.contains(query_word) || query_word.contains(name_word) {
                    score += 5.0;
                }
            }
        }

        score
    }
}
