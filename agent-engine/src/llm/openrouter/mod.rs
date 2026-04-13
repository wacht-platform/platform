use std::time::Duration;

use common::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tracing::info;

use crate::{
    json_schema::{normalize_json_schema, normalize_openai_tool_schema},
    llm::{
        GeneratedToolCall, NativeToolDefinition, PromptCacheRequest, SemanticLlmContentBlock,
        SemanticLlmMessage, SemanticLlmRequest, StructuredGenerationOutput,
        ToolCallGenerationOutput, UsageMetadata,
    },
};

const OPENROUTER_API_BASE_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
const REQUEST_TIMEOUT_SECS: u64 = 240;
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct OpenRouterClient {
    api_key: String,
    model: String,
    require_parameters: bool,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
struct OpenRouterResponse {
    choices: Vec<OpenRouterChoice>,
    #[serde(default)]
    usage: Option<OpenRouterUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterChoice {
    message: OpenRouterMessageResponse,
}

#[derive(Debug, Deserialize)]
struct OpenRouterMessageResponse {
    #[serde(default)]
    content: Option<OpenRouterMessageContent>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenRouterToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterToolCall {
    #[serde(rename = "type")]
    _tool_type: Option<String>,
    function: OpenRouterFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OpenRouterFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpenRouterMessageContent {
    Text(String),
    Parts(Vec<OpenRouterContentPart>),
}

#[derive(Debug, Deserialize)]
struct OpenRouterContentPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<OpenRouterPromptTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterPromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u32>,
    #[serde(default)]
    cache_write_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterErrorEnvelope {
    error: OpenRouterErrorBody,
}

#[derive(Debug, Deserialize)]
struct OpenRouterErrorBody {
    #[serde(default)]
    code: Option<Value>,
}

impl OpenRouterClient {
    fn parse_error_envelope(response_text: &str) -> Option<OpenRouterErrorEnvelope> {
        serde_json::from_str::<OpenRouterErrorEnvelope>(response_text).ok()
    }

    fn error_code_as_u64(code: &Value) -> Option<u64> {
        match code {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.parse::<u64>().ok(),
            _ => None,
        }
    }

    fn retry_delay_from_headers(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
        let retry_after = headers.get(reqwest::header::RETRY_AFTER)?;
        let retry_after = retry_after.to_str().ok()?.trim();
        let secs = retry_after.parse::<f64>().ok()?;
        Some(Duration::from_secs_f64(secs.max(0.5)))
    }

    fn should_retry_response(status_code: u16, response_text: &str) -> bool {
        if matches!(status_code, 408 | 409 | 429 | 502 | 503 | 504) {
            return true;
        }

        Self::parse_error_envelope(response_text)
            .and_then(|envelope| envelope.error.code)
            .as_ref()
            .and_then(Self::error_code_as_u64)
            .map(|code| matches!(code, 408 | 409 | 429 | 502 | 503 | 504 | 524))
            .unwrap_or(false)
    }

    pub fn new(api_key: String, model: String, require_parameters: bool) -> Self {
        Self {
            api_key,
            model,
            require_parameters,
            client: reqwest::Client::new(),
        }
    }

    pub fn from_api_key(
        deployment_api_key: Option<String>,
        model: &str,
        require_parameters: bool,
    ) -> Result<Self, AppError> {
        let Some(api_key) = deployment_api_key else {
            return Err(AppError::BadRequest(
                "OpenRouter API key is not configured for this deployment".to_string(),
            ));
        };
        Ok(Self::new(api_key, model.to_string(), require_parameters))
    }

    pub async fn generate_structured_from_prompt<T>(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<StructuredGenerationOutput<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let request_body = self.build_request_body(prompt, cache)?;
        info!(
            "{}",
            json!({
                "event": "openrouter_generate_request",
                "model": self.model,
                "url": OPENROUTER_API_BASE_URL,
                "request": request_body,
            })
            .to_string()
        );
        let parsed = self.execute_request(request_body).await?;

        let generated_text = parsed
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .map(Self::message_content_as_text)
            .unwrap_or_default();

        if generated_text.is_empty() {
            return Err(AppError::Internal(
                "No response content from OpenRouter API".to_string(),
            ));
        }

        let value = serde_json::from_str::<T>(&generated_text).map_err(|e| {
            AppError::Internal(format!(
                "Failed to parse OpenRouter generated content: {}. Generated text (first 2000 chars): {}",
                e,
                generated_text.chars().take(2000).collect::<String>()
            ))
        })?;

        Ok(StructuredGenerationOutput {
            value,
            usage_metadata: parsed.usage.map(Self::map_usage_metadata),
            cache_state: None,
        })
    }

    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        let request_body = self.build_tool_call_request_body(prompt, tools)?;
        info!(
            "{}",
            json!({
                "event": "openrouter_generate_request",
                "model": self.model,
                "url": OPENROUTER_API_BASE_URL,
                "request": request_body,
            })
            .to_string()
        );

        let parsed = self.execute_request(request_body).await?;
        let tool_calls = parsed
            .choices
            .first()
            .ok_or_else(|| AppError::Internal("OpenRouter returned no choices".to_string()))?
            .message
            .tool_calls
            .as_ref()
            .ok_or_else(|| AppError::Internal("OpenRouter returned no tool calls".to_string()))?;

        let calls = tool_calls
            .iter()
            .map(|call| {
                let arguments =
                    serde_json::from_str::<Value>(&call.function.arguments).map_err(|error| {
                        AppError::Internal(format!(
                            "Failed to parse OpenRouter tool arguments for {}: {}",
                            call.function.name, error
                        ))
                    })?;
                Ok(GeneratedToolCall {
                    tool_name: call.function.name.clone(),
                    arguments,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?;

        if calls.is_empty() {
            return Err(AppError::Internal(
                "OpenRouter returned an empty tool call list".to_string(),
            ));
        }

        Ok(ToolCallGenerationOutput {
            calls,
            usage_metadata: parsed.usage.map(Self::map_usage_metadata),
        })
    }

    fn build_request_body(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<Value, AppError> {
        let response_json_schema = normalize_json_schema(prompt.response_json_schema.clone());
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        messages.push(self.system_message(&prompt.system_prompt));
        messages.extend(
            prompt
                .messages
                .into_iter()
                .map(|message| self.semantic_message_to_openrouter(message)),
        );

        if let Some(cache_request) = cache.as_ref() {
            self.apply_cache_controls(&mut messages, cache_request);
        }

        let mut body = serde_json::Map::new();
        body.insert("model".to_string(), json!(self.model));
        body.insert("messages".to_string(), Value::Array(messages));
        if self.require_parameters {
            body.insert(
                "provider".to_string(),
                json!({
                    "require_parameters": true
                }),
            );
        }
        body.insert("stream".to_string(), json!(false));
        body.insert(
            "response_format".to_string(),
            json!({
                "type": "json_schema",
                "json_schema": {
                    "name": "structured_output",
                    "strict": true,
                    "schema": response_json_schema,
                }
            }),
        );
        if let Some(temperature) = prompt.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(max_output_tokens) = prompt.max_output_tokens {
            body.insert("max_tokens".to_string(), json!(max_output_tokens));
        }
        if let Some(reasoning_effort) = prompt.reasoning_effort {
            body.insert(
                "reasoning".to_string(),
                json!({
                    "effort": reasoning_effort,
                    "exclude": true,
                }),
            );
        }
        Ok(Value::Object(body))
    }

    fn build_tool_call_request_body(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
    ) -> Result<Value, AppError> {
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        messages.push(self.system_message(&prompt.system_prompt));
        messages.extend(
            prompt
                .messages
                .into_iter()
                .map(|message| self.semantic_message_to_openrouter(message)),
        );

        let tool_values = tools
            .into_iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "strict": true,
                        "parameters": normalize_openai_tool_schema(tool.input_schema),
                    }
                })
            })
            .collect::<Vec<_>>();

        let mut body = serde_json::Map::new();
        body.insert("model".to_string(), json!(self.model));
        body.insert("messages".to_string(), Value::Array(messages));
        body.insert("tools".to_string(), Value::Array(tool_values));
        body.insert("tool_choice".to_string(), json!("required"));
        if self.require_parameters {
            body.insert(
                "provider".to_string(),
                json!({
                    "require_parameters": true
                }),
            );
        }
        body.insert("stream".to_string(), json!(false));
        if let Some(temperature) = prompt.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(max_output_tokens) = prompt.max_output_tokens {
            body.insert("max_tokens".to_string(), json!(max_output_tokens));
        }
        if let Some(reasoning_effort) = prompt.reasoning_effort {
            body.insert(
                "reasoning".to_string(),
                json!({
                    "effort": reasoning_effort,
                    "exclude": true,
                }),
            );
        }
        Ok(Value::Object(body))
    }

    async fn execute_request(&self, request_body: Value) -> Result<OpenRouterResponse, AppError> {
        let mut attempt = 0u32;
        let response_text = loop {
            let response = match self
                .client
                .post(OPENROUTER_API_BASE_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .send()
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    let should_retry = e.is_timeout() || e.is_connect() || e.is_request();
                    attempt += 1;
                    if should_retry && attempt < MAX_RETRIES {
                        tokio::time::sleep(Self::calculate_backoff_delay(attempt)).await;
                        continue;
                    }
                    return Err(AppError::Internal(format!(
                        "OpenRouter request failed: {e}"
                    )));
                }
            };

            let status = response.status();
            let retry_delay = Self::retry_delay_from_headers(response.headers());
            let response_text = response.text().await.map_err(|e| {
                AppError::Internal(format!("Failed to read OpenRouter response body: {e}"))
            })?;

            info!(
                "{}",
                json!({
                    "event": "openrouter_generate_response",
                    "model": self.model,
                    "url": OPENROUTER_API_BASE_URL,
                    "status": status.as_u16(),
                    "ok": status.is_success(),
                    "response": response_text,
                })
                .to_string()
            );

            if status.is_success() && Self::parse_error_envelope(&response_text).is_none() {
                break response_text;
            }

            let should_retry = Self::should_retry_response(status.as_u16(), &response_text);
            attempt += 1;
            if should_retry && attempt < MAX_RETRIES {
                tokio::time::sleep(
                    retry_delay.unwrap_or_else(|| Self::calculate_backoff_delay(attempt)),
                )
                .await;
                continue;
            }

            return Err(AppError::Internal(format!(
                "OpenRouter request failed with status {}: {}",
                status, response_text
            )));
        };

        serde_json::from_str(&response_text).map_err(|e| {
            AppError::Internal(format!(
                "Failed to parse OpenRouter response JSON: {}. Raw body (first 2000 chars): {}",
                e,
                response_text.chars().take(2000).collect::<String>()
            ))
        })
    }

    fn calculate_backoff_delay(attempt: u32) -> Duration {
        let capped = attempt.min(5);
        Duration::from_millis(500 * (1u64 << capped.saturating_sub(1)))
    }

    fn system_message(&self, system_prompt: &str) -> Value {
        json!({
            "role": "system",
            "content": system_prompt,
        })
    }

    fn semantic_message_to_openrouter(&self, message: SemanticLlmMessage) -> Value {
        let role = match message.role.as_str() {
            "model" | "assistant" => "assistant",
            "system" => "system",
            "developer" => "developer",
            "user" => "user",
            _ => "user",
        };
        let content_blocks = message
            .content_blocks
            .into_iter()
            .map(|block| match block {
                SemanticLlmContentBlock::Text { text } => json!({
                    "type": "text",
                    "text": text,
                }),
                SemanticLlmContentBlock::InlineData { mime_type, data } => json!({
                    "type": "image_url",
                    "image_url": {
                        "url": format!("data:{};base64,{}", mime_type, data),
                    }
                }),
            })
            .collect::<Vec<_>>();

        let content = if content_blocks
            .iter()
            .all(|block| block.get("type").and_then(|value| value.as_str()) == Some("text"))
        {
            Value::String(
                content_blocks
                    .iter()
                    .filter_map(|block| block.get("text").and_then(|value| value.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        } else {
            Value::Array(content_blocks)
        };

        json!({
            "role": role,
            "content": content,
        })
    }

    fn apply_cache_controls(&self, messages: &mut [Value], cache_request: &PromptCacheRequest) {
        if !Self::supports_explicit_cache_controls(&self.model) {
            return;
        }

        if messages.is_empty() {
            return;
        }

        let cacheable_count = messages
            .len()
            .saturating_sub(cache_request.live_tail_count.min(messages.len()));
        if cacheable_count == 0 {
            return;
        }

        let Some(last_cacheable_message) = messages.get_mut(cacheable_count - 1) else {
            return;
        };
        let Some(content) = last_cacheable_message
            .get_mut("content")
            .and_then(|value| value.as_array_mut())
        else {
            return;
        };
        if content.is_empty() {
            return;
        }

        let target_index = content
            .iter()
            .rposition(|block| {
                block
                    .get("type")
                    .and_then(|value| value.as_str())
                    .map(|value| value == "text")
                    .unwrap_or(false)
            })
            .or_else(|| content.len().checked_sub(1));

        if let Some(last_cacheable_block) = target_index.and_then(|index| content.get_mut(index)) {
            last_cacheable_block["cache_control"] = json!({
                "type": "ephemeral"
            });
        }
    }

    fn supports_explicit_cache_controls(model: &str) -> bool {
        model.starts_with("anthropic/") || model.starts_with("google/")
    }

    fn message_content_as_text(content: &OpenRouterMessageContent) -> String {
        match content {
            OpenRouterMessageContent::Text(text) => text.clone(),
            OpenRouterMessageContent::Parts(parts) => parts
                .iter()
                .filter(|part| part.part_type.as_deref() == Some("text"))
                .filter_map(|part| part.text.as_ref())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    fn map_usage_metadata(usage: OpenRouterUsage) -> UsageMetadata {
        UsageMetadata {
            prompt_token_count: usage.prompt_tokens,
            cached_content_token_count: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
            candidates_token_count: usage.completion_tokens,
            total_token_count: usage.total_tokens,
            thoughts_token_count: None,
            tool_use_prompt_token_count: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cache_write_tokens),
            prompt_tokens_details: None,
            cache_tokens_details: None,
            candidates_tokens_details: None,
            tool_use_prompt_tokens_details: None,
        }
    }
}
