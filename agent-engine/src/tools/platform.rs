use super::ToolExecutor;
use common::error::AppError;
use common::ResultExt;
use dto::json::{ApiToolResult, PlatformEventResult, StreamEvent};
use models::{AiTool, ApiToolConfiguration, HttpMethod, PlatformEventToolConfiguration};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

impl ToolExecutor {
    pub(super) async fn execute_api_tool(
        &self,
        tool: &AiTool,
        config: &ApiToolConfiguration,
        execution_params: &Value,
    ) -> Result<ApiToolResult, AppError> {
        let url_param_names: HashSet<&str> = config
            .url_params_schema
            .as_ref()
            .map(|fields| fields.iter().map(|f| f.name.as_str()).collect())
            .unwrap_or_default();

        let mut url_params: HashMap<String, String> = HashMap::new();
        let mut body_map = serde_json::Map::new();
        if let Value::Object(map) = execution_params {
            for (key, value) in map {
                if key == "headers" {
                    continue;
                }
                if url_param_names.contains(key.as_str()) {
                    let value_str = match value {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => value.to_string(),
                    };
                    url_params.insert(key.clone(), value_str);
                } else {
                    body_map.insert(key.clone(), value.clone());
                }
            }
        }
        let body = if body_map.is_empty() {
            None
        } else {
            Some(Value::Object(body_map))
        };

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

        let timeout_secs = config.timeout_seconds.unwrap_or(30);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs as u64))
            .build()
            .map_err_internal("Failed to build HTTP client")?;

        let mut request_builder = match config.method {
            HttpMethod::GET => client.get(&url),
            HttpMethod::POST => client.post(&url),
            HttpMethod::PUT => client.put(&url),
            HttpMethod::PATCH => client.patch(&url),
            HttpMethod::DELETE => client.delete(&url),
        };

        request_builder = request_builder
            .header("X-Wacht-Thread-Id", self.ctx.thread_id.to_string())
            .header("X-Wacht-Actor-Id", self.ctx.actor_id.to_string());

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

    pub(super) async fn execute_platform_event_tool(
        &self,
        tool: &AiTool,
        config: &PlatformEventToolConfiguration,
        execution_params: &Value,
    ) -> Result<PlatformEventResult, AppError> {
        let event_data = execution_params
            .get("event_data")
            .cloned()
            .or_else(|| config.event_data.clone())
            .unwrap_or_else(common::json_utils::empty_object);

        if let Some(channel) = &self.channel {
            let event = StreamEvent::PlatformEvent(config.event_label.clone(), event_data.clone());
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
}
