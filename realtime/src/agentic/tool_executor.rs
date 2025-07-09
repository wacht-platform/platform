use serde_json::{Value, json};
use shared::commands::{Command, GenerateEmbeddingCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::error::AppError;
use shared::models::{AiTool, AiToolConfiguration};
use shared::models::{
    ApiToolConfiguration, KnowledgeBaseToolConfiguration, PlatformEventToolConfiguration,
    PlatformFunctionToolConfiguration,
};
use shared::models::{HttpMethod, ParameterValueType};
use shared::state::AppState;
use std::collections::HashMap;

pub struct ToolExecutor {
    app_state: AppState,
}

impl ToolExecutor {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn execute_tool_immediately(
        &self,
        tool: &AiTool,
        execution_params: Value,
    ) -> Result<Value, AppError> {
        match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                tracing::info!(
                    tool_name = %tool.name,
                    endpoint = %config.endpoint,
                    "ToolExecutor: Executing API tool"
                );
                self.execute_api_tool(tool, config, &execution_params).await
            }
            AiToolConfiguration::KnowledgeBase(config) => {
                tracing::info!(
                    tool_name = %tool.name,
                    knowledge_base_id = %config.knowledge_base_id,
                    "ToolExecutor: Executing Knowledge Base tool"
                );
                self.execute_knowledge_base_tool(tool, config, &execution_params)
                    .await
            }
            AiToolConfiguration::PlatformEvent(config) => {
                tracing::info!(
                    tool_name = %tool.name,
                    event_label = %config.event_label,
                    "ToolExecutor: Executing Platform Event tool"
                );
                self.execute_platform_event_tool(tool, config, &execution_params)
                    .await
            }
            AiToolConfiguration::PlatformFunction(config) => {
                tracing::info!(
                    tool_name = %tool.name,
                    function_name = %config.function_name,
                    "ToolExecutor: Executing Platform Function tool"
                );
                self.execute_platform_function_tool(tool, config, &execution_params)
                    .await
            }
        }
    }

    async fn execute_api_tool(
        &self,
        tool: &AiTool,
        config: &ApiToolConfiguration,
        params: &Value,
    ) -> Result<Value, AppError> {
        tracing::debug!(
            tool_name = %tool.name,
            params = %serde_json::to_string_pretty(params).unwrap_or_default(),
            "ToolExecutor: API tool parameters"
        );

        let mut url = config.endpoint.clone();

        if let Some(url_params) = params.get("url_params").and_then(|v| v.as_object()) {
            for (key, value) in url_params {
                let placeholder = format!("{{{}}}", key);
                url = url.replace(&placeholder, &value.as_str().unwrap_or(""));
            }
        }

        // Build query parameters
        let mut query_params = HashMap::new();
        for param in &config.query_parameters {
            let value = match &param.value_type {
                ParameterValueType::Hardcoded { value } => value.clone(),
                ParameterValueType::FromChat { lookup_key } => params
                    .get("query_params")
                    .and_then(|qp| qp.get(lookup_key))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            };
            if param.required || !value.is_empty() {
                query_params.insert(param.name.clone(), value);
            }
        }

        // Build headers
        let mut headers = Vec::new();
        headers.push(("Content-Type".to_string(), "application/json".to_string()));

        for header in &config.headers {
            let value = match &header.value_type {
                ParameterValueType::Hardcoded { value } => value.clone(),
                ParameterValueType::FromChat { lookup_key } => params
                    .get("headers")
                    .and_then(|h| h.get(lookup_key))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            };
            if !value.is_empty() {
                headers.push((header.name.clone(), value));
            }
        }

        // Handle authentication
        if let Some(auth_config) = &config.authorization {
            if auth_config.authorize_as_user {
                // Placeholder for token retrieval
                let token = self.retrieve_auth_token(tool.deployment_id).await?;
                headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
            }

            // Add custom auth headers
            for header in &auth_config.custom_headers {
                let value = match &header.value_type {
                    ParameterValueType::Hardcoded { value } => value.clone(),
                    ParameterValueType::FromChat { lookup_key } => params
                        .get("auth_headers")
                        .and_then(|h| h.get(lookup_key))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                };
                if !value.is_empty() {
                    headers.push((header.name.clone(), value));
                }
            }
        }

        // Build request body
        let body = if let Some(body_params) = params.get("body") {
            Some(body_params.clone())
        } else {
            None
        };

        // Build URL with query parameters
        let mut full_url = url.clone();
        if !query_params.is_empty() {
            let query_string: Vec<String> = query_params
                .iter()
                .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                .collect();
            full_url = format!("{}?{}", full_url, query_string.join("&"));
        }

        tracing::info!(
            tool_name = %tool.name,
            url = %full_url,
            has_body = %body.is_some(),
            headers_count = %headers.len(),
            "ToolExecutor: Making API request"
        );

        tracing::debug!(
            tool_name = %tool.name,
            headers = ?headers,
            "ToolExecutor: Request headers"
        );

        if let Some(ref body_data) = body {
            tracing::debug!(
                tool_name = %tool.name,
                body = %serde_json::to_string_pretty(body_data).unwrap_or_default(),
                "ToolExecutor: Request body"
            );
        }

        // Execute the request based on method
        let mut response = match config.method {
            HttpMethod::GET => {
                let mut req = ureq::get(&full_url);
                for (name, value) in &headers {
                    req = req.header(name, value);
                }
                req.call()
                    .map_err(|e| AppError::Internal(format!("API request failed: {}", e)))?
            }
            HttpMethod::POST => {
                let mut req = ureq::post(&full_url);
                for (name, value) in &headers {
                    req = req.header(name, value);
                }
                if let Some(body) = body.clone() {
                    req.send_json(&body)
                } else {
                    req.send(&[] as &[u8])
                }
                .map_err(|e| AppError::Internal(format!("API request failed: {}", e)))?
            }
            HttpMethod::PUT => {
                let mut req = ureq::put(&full_url);
                for (name, value) in &headers {
                    req = req.header(name, value);
                }
                if let Some(body) = body.clone() {
                    req.send_json(&body)
                } else {
                    req.send(&[] as &[u8])
                }
                .map_err(|e| AppError::Internal(format!("API request failed: {}", e)))?
            }
            HttpMethod::DELETE => {
                let mut req = ureq::delete(&full_url);
                for (name, value) in &headers {
                    req = req.header(name, value);
                }
                req.call()
                    .map_err(|e| AppError::Internal(format!("API request failed: {}", e)))?
            }
            HttpMethod::PATCH => {
                let mut req = ureq::patch(&full_url);
                for (name, value) in &headers {
                    req = req.header(name, value);
                }
                if let Some(body) = body.clone() {
                    req.send_json(&body)
                } else {
                    req.send(&[] as &[u8])
                }
                .map_err(|e| AppError::Internal(format!("API request failed: {}", e)))?
            }
        };

        let status = response.status();
        let response_text = response.body_mut().read_to_string().unwrap_or_default();

        println!("{response_text}");

        tracing::debug!(
            tool_name = %tool.name,
            status_code = %status.as_u16(),
            response_text = %response_text,
            "ToolExecutor: API response received"
        );

        let result = json!({
            "tool_id": tool.id,
            "tool_name": tool.name,
            "tool_type": "api",
            "status_code": status.as_u16(),
            "success": status.is_success(),
            "response": response_text,
            "execution_timestamp": chrono::Utc::now().to_rfc3339()
        });

        tracing::info!(
            tool_name = %tool.name,
            status_code = %status.as_u16(),
            success = %status.is_success(),
            "ToolExecutor: API tool execution completed"
        );

        tracing::debug!(
            tool_name = %tool.name,
            result = %serde_json::to_string_pretty(&result).unwrap_or_default(),
            "ToolExecutor: API tool final result"
        );

        Ok(result)
    }

    async fn execute_knowledge_base_tool(
        &self,
        tool: &AiTool,
        config: &KnowledgeBaseToolConfiguration,
        params: &Value,
    ) -> Result<Value, AppError> {
        tracing::debug!(
            tool_name = %tool.name,
            params = %serde_json::to_string_pretty(params).unwrap_or_default(),
            "ToolExecutor: Knowledge Base tool parameters"
        );

        // Extract query from parameters
        let query = params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::BadRequest(
                    "Query parameter required for knowledge base search".to_string(),
                )
            })?;

        tracing::info!(
            tool_name = %tool.name,
            knowledge_base_id = %config.knowledge_base_id,
            query = %query,
            "ToolExecutor: Searching knowledge base"
        );

        // Generate embedding for the query
        let query_embedding = GenerateEmbeddingCommand::new(query.to_string())
            .execute(&self.app_state)
            .await?;

        // Search the knowledge base
        let search_results = SearchKnowledgeBaseEmbeddingsCommand::new(
            config.knowledge_base_id,
            query_embedding,
            config.search_settings.max_results.unwrap_or(10) as u64,
        )
        .execute(&self.app_state)
        .await?;

        // Filter by similarity threshold if configured
        let filtered_results: Vec<_> =
            if let Some(threshold) = config.search_settings.similarity_threshold {
                search_results
                    .into_iter()
                    .filter(|r| r.score <= ((1.0 - threshold) * 2.0) as f64) // Convert similarity to L2 distance
                    .collect()
            } else {
                search_results
            };

        // Format results
        let mut formatted_results = Vec::new();
        for result in filtered_results {
            let mut item = json!({
                "content": result.content,
                "score": 1.0 - (result.score / 2.0), // Convert L2 distance back to similarity
                "document_id": result.document_id,
                "chunk_index": result.chunk_index,
            });

            if config.search_settings.include_metadata {
                item["knowledge_base_id"] = json!(result.knowledge_base_id);
            }

            formatted_results.push(item);
        }

        // Sort by relevance if configured
        if config.search_settings.sort_by_relevance {
            formatted_results.sort_by(|a, b| {
                let score_a = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let score_b = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
                score_b
                    .partial_cmp(&score_a)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        Ok(json!({
            "tool_id": tool.id,
            "tool_name": tool.name,
            "tool_type": "knowledge_base",
            "query": query,
            "results": formatted_results,
            "result_count": formatted_results.len(),
            "execution_timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    async fn execute_platform_event_tool(
        &self,
        tool: &AiTool,
        config: &PlatformEventToolConfiguration,
        params: &Value,
    ) -> Result<Value, AppError> {
        // Merge configured event data with runtime parameters
        let mut event_data = config.event_data.clone().unwrap_or(json!({}));

        if let Some(runtime_data) = params.get("event_data") {
            if let (Some(config_obj), Some(runtime_obj)) =
                (event_data.as_object_mut(), runtime_data.as_object())
            {
                for (key, value) in runtime_obj {
                    config_obj.insert(key.clone(), value.clone());
                }
            }
        }

        Ok(json!({
            "tool_id": tool.id,
            "tool_name": tool.name,
            "tool_type": "platform_event",
            "event_label": config.event_label.clone(),
            "event_data": event_data,
            "status": "event_triggered",
            "execution_timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    async fn execute_platform_function_tool(
        &self,
        tool: &AiTool,
        config: &PlatformFunctionToolConfiguration,
        params: &Value,
    ) -> Result<Value, AppError> {
        // Extract function inputs from parameters
        let default_inputs = json!({});
        let inputs = params.get("inputs").unwrap_or(&default_inputs);

        // Validate inputs against schema if provided
        if let Some(input_schema) = &config.input_schema {
            for field in input_schema {
                if field.required && !inputs.get(&field.name).is_some() {
                    return Err(AppError::BadRequest(format!(
                        "Required input field '{}' is missing",
                        field.name
                    )));
                }
            }
        }

        let result = match config.function_name.as_str() {
            _ => {
                json!({
                    "status": "function_not_implemented",
                    "function_name": config.function_name,
                    "message": format!("Platform function '{}' is not yet implemented", config.function_name)
                })
            }
        };

        Ok(json!({
            "tool_id": tool.id,
            "tool_name": tool.name,
            "tool_type": "platform_function",
            "function_name": config.function_name.clone(),
            "inputs": inputs,
            "result": result,
            "execution_timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    // Placeholder for authentication token retrieval
    async fn retrieve_auth_token(&self, _deployment_id: i64) -> Result<String, AppError> {
        // TODO: Implement actual token retrieval logic
        // This might involve:
        // 1. Checking for cached tokens
        // 2. Refreshing expired tokens
        // 3. Fetching from a token service
        // 4. Using deployment-specific credentials

        Err(AppError::Internal(
            "Authentication token retrieval not yet implemented".to_string(),
        ))
    }
}
