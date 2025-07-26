use crate::agentic::SharedExecutionContext;
use serde_json::{Value, json};
use shared::error::AppError;
use shared::models::HttpMethod;
use shared::models::{AiTool, AiToolConfiguration};
use shared::models::{
    ApiToolConfiguration, PlatformEventToolConfiguration,
    PlatformFunctionToolConfiguration,
};
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
            AiToolConfiguration::KnowledgeBase(_config) => {
                // TODO: Implement knowledge base tool execution
                Err(AppError::Internal("Knowledge base tool execution not implemented".to_string()))
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

        println!("API Tool - Original URL: {}", url);

        for (key, value) in &url_params {
            let placeholder = format!("{{{}}}", key);
            if url.contains(&placeholder) {
                println!("API Tool - Replacing {} with {}", placeholder, value);
                url = url.replace(&placeholder, value);
            } else {
                query_params.insert(key.clone(), value.clone());
            }
        }

        println!("API Tool - Final URL after substitution: {}", url);

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
            println!("API Tool - Adding query parameters: {:?}", query_params);
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


    async fn execute_platform_event_tool(
        &self,
        tool: &AiTool,
        _config: &PlatformEventToolConfiguration,
        execution_params: &Value,
    ) -> Result<Value, AppError> {
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
        let mut function_params = HashMap::new();

        if let Some(schema) = &config.input_schema {
            for field in schema {
                if let Some(value) = execution_params.get(&field.name) {
                    function_params.insert(field.name.clone(), value.clone());
                }
            }
        }

        Ok(json!({
            "success": true,
            "tool": tool.name,
            "function": config.function_name,
            "parameters": function_params,
            "message": "Platform function executed successfully",
        }))
    }
}
