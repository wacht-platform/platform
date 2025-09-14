use commands::{Command, GenerateEmbeddingsCommand, SearchKnowledgeBaseEmbeddingsCommand};
use common::error::AppError;
use common::state::AppState;
use dto::json::{
    ApiToolResult, KnowledgeBaseToolResult, PlatformEventResult, PlatformFunctionData,
    PlatformFunctionResult, StreamEvent, ToolKnowledgeBaseSearchResult,
};
use models::HttpMethod;
use models::{AiTool, AiToolConfiguration};
use models::{
    ApiToolConfiguration, KnowledgeBaseToolConfiguration, PlatformEventToolConfiguration,
    PlatformFunctionToolConfiguration,
};
use serde_json::Value;
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
                let result = self
                    .execute_api_tool(tool, config, &execution_params)
                    .await?;
                Ok(serde_json::to_value(result)?)
            }
            AiToolConfiguration::KnowledgeBase(config) => {
                let result = self
                    .execute_knowledge_base_tool(tool, config, &execution_params)
                    .await?;
                Ok(serde_json::to_value(result)?)
            }
            AiToolConfiguration::PlatformEvent(config) => {
                let result = self
                    .execute_platform_event_tool(tool, config, &execution_params)
                    .await?;
                Ok(serde_json::to_value(result)?)
            }
            AiToolConfiguration::PlatformFunction(config) => {
                let result = self
                    .execute_platform_function_tool(tool, config, &execution_params)
                    .await?;
                Ok(serde_json::to_value(result)?)
            }
        }
    }

    async fn execute_api_tool(
        &self,
        tool: &AiTool,
        config: &ApiToolConfiguration,
        execution_params: &Value,
    ) -> Result<ApiToolResult, AppError> {
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

        for (key, value) in &url_params {
            let placeholder = format!("{{{key}}}");
            if url.contains(&placeholder) {
                url = url.replace(&placeholder, value);
            } else {
                query_params.insert(key.clone(), value.clone());
            }
        }

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
            request_builder = request_builder.query(&query_params);
        }

        match config.method {
            HttpMethod::POST | HttpMethod::PUT | HttpMethod::PATCH => {
                request_builder = request_builder.header("Content-Type", "application/json");
                if let Some(body_value) = body {
                    request_builder = request_builder.json(&body_value);
                } else {
                    request_builder = request_builder.json(&serde_json::json!({}));
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
                    Ok(ApiToolResult {
                        success: true,
                        status,
                        data: Some(
                            serde_json::from_str::<Value>(&body_text)
                                .unwrap_or(Value::String(body_text)),
                        ),
                        error: None,
                        tool: tool.name.clone(),
                    })
                } else {
                    Ok(ApiToolResult {
                        success: false,
                        status,
                        data: None,
                        error: Some(body_text),
                        tool: tool.name.clone(),
                    })
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
    ) -> Result<PlatformEventResult, AppError> {
        // Get event data from execution params or use config default
        let event_data = execution_params
            .get("event_data")
            .cloned()
            .or_else(|| config.event_data.clone())
            .unwrap_or(serde_json::json!({}));

        // Emit the event via WebSocket if channel is available
        if let Some(channel) = &self.channel {
            let event = StreamEvent::PlatformEvent(config.event_label.clone(), event_data.clone());

            // Try to send, but don't fail if channel is closed
            let _ = channel.send(event).await;
        }

        Ok(PlatformEventResult {
            success: true,
            tool: tool.name.clone(),
            event_label: config.event_label.clone(),
            event_data,
            message: "Platform event emitted successfully".to_string(),
        })
    }

    async fn execute_platform_function_tool(
        &self,
        tool: &AiTool,
        config: &PlatformFunctionToolConfiguration,
        execution_params: &Value,
    ) -> Result<PlatformFunctionResult, AppError> {
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
        let function_data = PlatformFunctionData {
            execution_id: execution_id.to_string(),
            function_name: config.function_name.clone(),
            parameters: function_params.clone(),
            is_overridable: config.is_overridable,
        };

        // Emit the platform function event via WebSocket if channel is available
        if let Some(channel) = &self.channel {
            let event = StreamEvent::PlatformFunction(
                config.function_name.clone(),
                serde_json::to_value(&function_data)?,
            );

            // Send the event
            let _ = channel.send(event).await;
        }

        // Return immediately with pending status
        Ok(PlatformFunctionResult {
            success: true,
            tool: tool.name.clone(),
            function: config.function_name.clone(),
            execution_id: execution_id.to_string(),
            status: "pending".to_string(),
        })
    }

    async fn execute_knowledge_base_tool(
        &self,
        tool: &AiTool,
        config: &KnowledgeBaseToolConfiguration,
        execution_params: &Value,
    ) -> Result<KnowledgeBaseToolResult, AppError> {
        // Get the query from execution parameters
        let query = execution_params
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AppError::Internal(
                    "Query parameter is required for knowledge base search".to_string(),
                )
            })?;

        // First generate embeddings for the query
        let embeddings_command = GenerateEmbeddingsCommand::new(vec![query.to_string()]);
        let embeddings = embeddings_command.execute(&self.app_state).await?;
        let query_embedding = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("Failed to generate query embedding".to_string()))?;

        // Search across all configured knowledge bases using semantic search
        let limit = config.search_settings.max_results.unwrap_or(10) as u64;
        let search_command = SearchKnowledgeBaseEmbeddingsCommand::new(
            config.knowledge_base_ids.clone(),
            query_embedding,
            limit,
        );

        let search_results = search_command.execute(&self.app_state).await?;

        // Filter by similarity threshold and convert to structs
        let threshold = config.search_settings.similarity_threshold.unwrap_or(0.7);
        let mut all_results: Vec<ToolKnowledgeBaseSearchResult> = search_results
            .into_iter()
            .filter(|result| result.score >= threshold as f64)
            .map(|result| ToolKnowledgeBaseSearchResult {
                content: result.content,
                knowledge_base_id: result.knowledge_base_id.to_string(),
                similarity_score: result.score,
                chunk_index: result.chunk_index,
                document_id: result.document_id.to_string(),
                document_title: result.document_title,
                document_description: result.document_description,
            })
            .collect();

        // Sort all results by relevance if requested
        if config.search_settings.sort_by_relevance {
            all_results.sort_by(|a, b| {
                b.similarity_score
                    .partial_cmp(&a.similarity_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        // Limit total results
        let max_results = config.search_settings.max_results.unwrap_or(10) as usize;
        all_results.truncate(max_results);

        Ok(KnowledgeBaseToolResult {
            success: true,
            tool: tool.name.clone(),
            query: query.to_string(),
            knowledge_base_ids: config.knowledge_base_ids.clone(),
            results: all_results.clone(),
            total_results: all_results.len(),
            search_settings: serde_json::to_value(&config.search_settings)?,
        })
    }
}
