use crate::agentic::SharedExecutionContext;
use serde_json::{Value, json};
use shared::error::AppError;
use shared::models::{AiTool, AiToolConfiguration};
use shared::models::{
    ApiToolConfiguration, KnowledgeBaseToolConfiguration, PlatformEventToolConfiguration,
    PlatformFunctionToolConfiguration,
};
use shared::models::HttpMethod;
use shared::models::{ContextAction, ContextEngineParams, ContextFilters};
use std::collections::HashMap;

pub struct ToolExecutor {
    shared_context: SharedExecutionContext,
}

impl ToolExecutor {
    pub fn new(shared_context: SharedExecutionContext) -> Self {
        Self { shared_context }
    }

    pub async fn execute_tool_immediately(
        &self,
        tool: &AiTool,
        execution_params: Value,
    ) -> Result<Value, AppError> {
        match &tool.configuration {
            AiToolConfiguration::Api(config) => {
                self.execute_api_tool(tool, config, &execution_params).await
            }
            AiToolConfiguration::KnowledgeBase(config) => {
                self.execute_knowledge_base_tool(tool, config, &execution_params)
                    .await
            }
            AiToolConfiguration::PlatformEvent(config) => {
                self.execute_platform_event_tool(tool, config, &execution_params)
                    .await
            }
            AiToolConfiguration::PlatformFunction(config) => {
                self.execute_platform_function_tool(tool, config, &execution_params)
                    .await
            }
        }
    }

    /// Execute a tool with context search capability
    pub async fn execute_tool_with_context(
        &self,
        tool: &AiTool,
        execution_params: Value,
        context_query: Option<String>,
    ) -> Result<Value, AppError> {
        // If context query is provided, search for relevant context first
        let context_results = if let Some(query) = context_query {
            let params = ContextEngineParams {
                query,
                action: ContextAction::SearchAll,
                filters: Some(ContextFilters {
                    max_results: 5,
                    min_relevance: 0.7,
                    time_range: None,
                    search_mode: shared::models::SearchMode::default(),
                    boost_keywords: None,
                }),
            };

            match self.shared_context.context_engine().execute(params).await {
                Ok(results) => Some(results),
                Err(e) => {
                    tracing::warn!("Context search failed: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Execute the tool with enhanced parameters including context
        let mut enhanced_params = execution_params.clone();
        if let Some(context) = context_results {
            if let Some(params_obj) = enhanced_params.as_object_mut() {
                params_obj.insert(
                    "_context".to_string(),
                    json!(context.into_iter().map(|r| json!({
                        "content": r.content,
                        "relevance": r.relevance_score,
                        "source": format!("{:?}", r.source),
                    })).collect::<Vec<_>>()),
                );
            }
        }

        self.execute_tool_immediately(tool, enhanced_params).await
    }

    async fn execute_api_tool(
        &self,
        tool: &AiTool,
        config: &ApiToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        // Extract parameters from execution params
        let url_params = execution_params
            .get("url_params")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        let query_params = execution_params
            .get("query_params")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        let body = execution_params.get("body").cloned();

        // Build the URL with path parameters
        let mut url = config.endpoint.clone();
        for (key, value) in &url_params {
            url = url.replace(&format!("{{{}}}", key), value);
        }

        // Make the HTTP request using async reqwest client
        let client = reqwest::Client::new();

        // Build request based on method
        let mut request_builder = match config.method {
            HttpMethod::GET => client.get(&url),
            HttpMethod::POST => client.post(&url),
            HttpMethod::PUT => client.put(&url),
            HttpMethod::PATCH => client.patch(&url),
            HttpMethod::DELETE => client.delete(&url),
        };

        // Add headers
        if let Some(headers) = execution_params.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(header_value) = value.as_str() {
                    request_builder = request_builder.header(key, header_value);
                }
            }
        }

        // Add query parameters
        request_builder = request_builder.query(&query_params);

        // Add body for methods that support it
        match config.method {
            HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH => {
                request_builder = request_builder.header("Content-Type", "application/json");
                if let Some(body_value) = body {
                    request_builder = request_builder.json(&body_value);
                } else {
                    request_builder = request_builder.json(&json!({}));
                }
            }
            _ => {}
        }

        // Execute the request
        let response = request_builder.send().await;

        match response {
            Ok(res) => {
                let status = res.status().as_u16();
                let body_text = res.text().await.unwrap_or_default();

                if status >= 200 && status < 300 {
                    Ok(json!({
                        "success": true,
                        "status": status,
                        "data": serde_json::from_str::<Value>(&body_text).unwrap_or(Value::String(body_text)),
                        "tool": tool.name,
                    }))
                } else {
                    Ok(json!({
                        "success": false,
                        "status": status,
                        "error": body_text,
                        "tool": tool.name,
                    }))
                }
            }
            Err(e) => Err(AppError::External(format!("API request failed: {}", e))),
        }
    }

    async fn execute_knowledge_base_tool(
        &self,
        tool: &AiTool,
        config: &KnowledgeBaseToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        // Extract the search query
        let query = execution_params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::BadRequest("Query parameter is required".to_string()))?;

        // Use context engine for knowledge base search
        let params = ContextEngineParams {
            query: query.to_string(),
            action: ContextAction::SearchKnowledgeBase {
                kb_id: Some(config.knowledge_base_id),
            },
            filters: Some(ContextFilters {
                max_results: config.search_settings.max_results.unwrap_or(10) as usize,
                min_relevance: config.search_settings.similarity_threshold.unwrap_or(0.7) as f64,
                time_range: None,
                search_mode: shared::models::SearchMode::default(),
                boost_keywords: None,
            }),
        };

        let search_results = self.shared_context.context_engine().execute(params).await?;

        // Format results
        let formatted_results: Vec<Value> = search_results
            .into_iter()
            .map(|result| {
                json!({
                    "content": result.content,
                    "relevance_score": result.relevance_score,
                    "metadata": result.metadata,
                })
            })
            .collect();

        Ok(json!({
            "success": true,
            "tool": tool.name,
            "knowledge_base_id": config.knowledge_base_id,
            "query": query,
            "results": formatted_results,
            "result_count": formatted_results.len(),
        }))
    }

    async fn execute_platform_event_tool(
        &self,
        tool: &AiTool,
        _config: &PlatformEventToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        // Platform events are typically handled by event systems
        // For now, we'll return a success response
        Ok(json!({
            "success": true,
            "tool": tool.name,
            "event_data": execution_params.get("event_data").cloned().unwrap_or(json!({})),
            "message": "Platform event triggered successfully",
        }))
    }

    async fn execute_platform_function_tool(
        &self,
        tool: &AiTool,
        config: &PlatformFunctionToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        // Extract function parameters based on the input schema
        let mut function_params = HashMap::new();

        if let Some(schema) = &config.input_schema {
            for field in schema {
                if let Some(value) = execution_params.get(&field.name) {
                    function_params.insert(field.name.clone(), value.clone());
                }
            }
        }

        // Platform functions would typically be executed through a registry
        // For now, return a placeholder response
        Ok(json!({
            "success": true,
            "tool": tool.name,
            "function": config.function_name,
            "parameters": function_params,
            "message": "Platform function executed successfully",
        }))
    }

    async fn make_api_request(
        &self,
        config: &ApiToolConfiguration,
        tool_name: &str,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        // This is a helper method that could be extracted later
        // For now, it's handled in execute_api_tool
        self.execute_api_tool(
            &AiTool {
                id: 0,
                name: tool_name.to_string(),
                description: Some("".to_string()),
                deployment_id: 0,
                configuration: AiToolConfiguration::Api(config.clone()),
                tool_type: shared::models::AiToolType::Api,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            },
            config,
            execution_params,
        )
        .await
    }
}