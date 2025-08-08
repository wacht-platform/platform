use serde_json::{Value, json};
use shared::error::AppError;
use shared::models::HttpMethod;
use shared::models::{AiTool, AiToolConfiguration};
use shared::models::{
    ApiToolConfiguration, KnowledgeBaseToolConfiguration, PlatformEventToolConfiguration, PlatformFunctionToolConfiguration,
};
use shared::state::AppState;
use shared::commands::{Command, GenerateEmbeddingsCommand, SearchKnowledgeBaseEmbeddingsCommand};
use shared::dto::json::StreamEvent;
use std::collections::HashMap;

pub struct ToolExecutor {
    app_state: AppState,
    channel: Option<tokio::sync::mpsc::Sender<StreamEvent>>,
}

impl ToolExecutor {
    pub fn new(app_state: AppState) -> Self {
        Self { 
            app_state,
            channel: None,
        }
    }
    
    pub fn with_channel(mut self, channel: tokio::sync::mpsc::Sender<StreamEvent>) -> Self {
        self.channel = Some(channel);
        self
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
                self.execute_knowledge_base_tool(tool, config, &execution_params).await
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

    async fn execute_api_tool(
        &self,
        tool: &AiTool,
        config: &ApiToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let url_params = execution_params
            .get("url_params")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| {
                        let value_str = match v {
                            Value::String(s) => s.clone(),
                            Value::Number(n) => n.to_string(),
                            Value::Bool(b) => b.to_string(),
                            _ => v.to_string(),
                        };
                        (k.clone(), value_str)
                    })
                    .collect::<HashMap<String, String>>()
            })
            .unwrap_or_default();

        let body = execution_params.get("body").cloned();

        let mut url = config.endpoint.clone();
        let mut query_params = HashMap::new();

        println!("API Tool - Original URL: {url}");

        for (key, value) in &url_params {
            let placeholder = format!("{{{key}}}");
            if url.contains(&placeholder) {
                println!("API Tool - Replacing {placeholder} with {value}");
                url = url.replace(&placeholder, value);
            } else {
                query_params.insert(key.clone(), value.clone());
            }
        }

        println!("API Tool - Final URL after substitution: {url}");

        let client = reqwest::Client::new();

        let mut request_builder = match config.method {
            HttpMethod::GET => client.get(&url),
            HttpMethod::POST => client.post(&url),
            HttpMethod::PUT => client.put(&url),
            HttpMethod::PATCH => client.patch(&url),
            HttpMethod::DELETE => client.delete(&url),
        };

        if let Some(headers) = execution_params.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(header_value) = value.as_str() {
                    request_builder = request_builder.header(key, header_value);
                }
            }
        }

        if !query_params.is_empty() {
            println!("API Tool - Adding query parameters: {query_params:?}");
        }
        request_builder = request_builder.query(&query_params);

        match config.method {
            HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH => {
                request_builder = request_builder.header("Content-Type", "application/json");
                if let Some(body_value) = body {
                    println!(
                        "API Tool - Adding request body: {}",
                        serde_json::to_string_pretty(&body_value)
                            .unwrap_or_else(|_| "Invalid JSON".to_string())
                    );
                    request_builder = request_builder.json(&body_value);
                } else {
                    println!("API Tool - No body provided, sending empty object");
                    request_builder = request_builder.json(&json!({}));
                }
            }
            _ => {}
        }

        let response = request_builder.send().await;

        match response {
            Ok(res) => {
                let status = res.status().as_u16();
                let body_text = res.text().await.unwrap_or_default();

                if (200..300).contains(&status) {
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
            Err(e) => Err(AppError::External(format!("API request failed: {e}"))),
        }
    }

    async fn execute_platform_event_tool(
        &self,
        tool: &AiTool,
        config: &PlatformEventToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        // Get event data from execution params or use config default
        let event_data = execution_params
            .get("event_data")
            .cloned()
            .or_else(|| config.event_data.clone())
            .unwrap_or(json!({}));
        
        // Emit the event via WebSocket if channel is available
        if let Some(channel) = &self.channel {
            let event = StreamEvent::PlatformEvent(
                config.event_label.clone(),
                event_data.clone(),
            );
            
            // Try to send, but don't fail if channel is closed
            let _ = channel.send(event).await;
        }
        
        Ok(json!({
            "success": true,
            "tool": tool.name,
            "event_label": config.event_label,
            "event_data": event_data,
            "message": "Platform event emitted successfully",
        }))
    }

    async fn execute_platform_function_tool(
        &self,
        tool: &AiTool,
        config: &PlatformFunctionToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        let mut function_params = HashMap::new();

        if let Some(schema) = &config.input_schema {
            for field in schema {
                if let Some(value) = execution_params.get(&field.name) {
                    function_params.insert(field.name.clone(), value.clone());
                }
            }
        }

        // Generate a unique execution ID for this function call
        let execution_id = self.app_state.sf.next_id()? as u64;
        
        // Prepare function data - send execution_id as string to avoid JS number precision issues
        let function_data = json!({
            "execution_id": execution_id.to_string(),
            "function_name": config.function_name,
            "parameters": function_params,
            "is_overridable": config.is_overridable,
        });

        // Emit the platform function event via WebSocket if channel is available
        if let Some(channel) = &self.channel {
            tracing::info!(
                "Sending platform function to frontend: function_name: {}, execution_id: {}, params: {:?}",
                config.function_name,
                execution_id,
                function_params
            );
            
            let event = StreamEvent::PlatformFunction(
                config.function_name.clone(),
                function_data.clone(),
            );
            
            // Send the event
            let send_result = channel.send(event).await;
            tracing::info!(
                "Platform function send result: {:?}",
                send_result.is_ok()
            );
        } else {
            tracing::warn!("No channel available to send platform function");
        }

        // Return immediately with pending status
        Ok(json!({
            "success": true,
            "tool": tool.name,
            "function": config.function_name,
            "execution_id": execution_id.to_string(),
            "status": "pending"
        }))
    }

    async fn execute_knowledge_base_tool(
        &self,
        tool: &AiTool,
        config: &KnowledgeBaseToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
        // Get the query from execution parameters
        let query = execution_params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::Internal("Query parameter is required for knowledge base search".to_string()))?;

        // First generate embeddings for the query
        let embeddings_command = GenerateEmbeddingsCommand::new(vec![query.to_string()]);
        let embeddings = embeddings_command.execute(&self.app_state).await?;
        let query_embedding = embeddings.into_iter().next()
            .ok_or_else(|| AppError::Internal("Failed to generate query embedding".to_string()))?;

        // Search across all configured knowledge bases using semantic search
        let limit = config.search_settings.max_results.unwrap_or(10) as u64;
        let search_command = SearchKnowledgeBaseEmbeddingsCommand::new(
            config.knowledge_base_ids.clone(),
            query_embedding,
            limit,
        );
        
        let search_results = search_command.execute(&self.app_state).await?;
        
        // Filter by similarity threshold and convert to JSON
        let threshold = config.search_settings.similarity_threshold.unwrap_or(0.7);
        let mut all_results: Vec<Value> = search_results
            .into_iter()
            .filter(|result| result.score >= threshold as f64)
            .map(|result| json!({
                "content": result.content,
                "knowledge_base_id": result.knowledge_base_id.to_string(),
                "similarity_score": result.score,
                "chunk_index": result.chunk_index,
                "document_id": result.document_id.to_string(),
                "document_title": result.document_title,
                "document_description": result.document_description,
            }))
            .collect();

        // Sort all results by relevance if requested
        if config.search_settings.sort_by_relevance {
            all_results.sort_by(|a, b| {
                let score_a = a.get("similarity_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let score_b = b.get("similarity_score")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Limit total results
        let max_results = config.search_settings.max_results.unwrap_or(10) as usize;
        all_results.truncate(max_results);

        Ok(json!({
            "success": true,
            "tool": tool.name,
            "query": query,
            "knowledge_base_ids": config.knowledge_base_ids,
            "results": all_results,
            "total_results": all_results.len(),
            "search_settings": config.search_settings,
        }))
    }
}
