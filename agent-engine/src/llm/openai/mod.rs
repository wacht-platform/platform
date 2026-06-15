use std::time::Duration;

use common::error::AppError;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    json_schema::{
        normalize_openai_response_schema, normalize_openai_tool_schema, schema_has_free_form_object,
    },
    llm::{
        GeneratedToolCall, NativeToolDefinition, PromptCacheRequest, SemanticLlmContentBlock,
        SemanticLlmMessage, SemanticLlmRequest, StructuredGenerationOutput,
        ToolCallGenerationOutput, UsageMetadata,
    },
};

const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";
const REQUEST_TIMEOUT_SECS: u64 = 240;
// RETRY POLICY (explicit product requirement):
// - Up to 15 attempts on retryable failures (timeouts, 5xx, 429, transient parse).
// - Fixed random delay between attempts, uniformly in [2s, 4s].
// - No exponential backoff. Do not honor Retry-After; the fixed window is the contract.
const MAX_RETRIES: u32 = 15;

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    api_key: String,
    model: String,
    chat_completions_url: String,
    organization: Option<String>,
    project: Option<String>,
    client: reqwest::Client,
    deployment_id: Option<i64>,
    thread_id: Option<i64>,
    actor_id: Option<i64>,
    nats_client: Option<async_nats::Client>,
    is_byok: bool,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessageResponse,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessageResponse {
    #[serde(default)]
    content: Option<OpenAiMessageContent>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    #[serde(rename = "type")]
    _tool_type: Option<String>,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OpenAiMessageContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
}

#[derive(Debug, Deserialize)]
struct OpenAiContentPart {
    #[serde(rename = "type")]
    part_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    total_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<OpenAiPromptTokensDetails>,
    #[serde(default)]
    completion_tokens_details: Option<OpenAiCompletionTokensDetails>,
}

#[derive(Debug, Deserialize)]
struct OpenAiPromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u32>,
    #[serde(default)]
    _audio_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: Option<u32>,
    #[serde(default)]
    _audio_tokens: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorEnvelope {
    error: OpenAiErrorBody,
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorBody {
    #[serde(default)]
    code: Option<Value>,
}

impl OpenAiClient {
    pub fn from_api_key(deployment_api_key: Option<String>, model: &str) -> Result<Self, AppError> {
        let Some(api_key) = deployment_api_key else {
            return Err(AppError::BadRequest(
                "OpenAI API key is not configured for this deployment".to_string(),
            ));
        };

        Ok(Self::new(api_key, model.to_string()))
    }

    pub fn from_profile(
        profile_api_key: Option<String>,
        model: &str,
        base_url: Option<String>,
        organization: Option<String>,
        project: Option<String>,
    ) -> Result<Self, AppError> {
        let Some(api_key) = profile_api_key else {
            return Err(AppError::BadRequest(
                "OpenAI API key is not configured for this provider profile".to_string(),
            ));
        };

        Ok(Self::new(api_key, model.to_string())
            .with_base_url(base_url)
            .with_headers(organization, project))
    }

    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            chat_completions_url: OPENAI_CHAT_COMPLETIONS_URL.to_string(),
            organization: None,
            project: None,
            client: reqwest::Client::new(),
            deployment_id: None,
            thread_id: None,
            actor_id: None,
            nats_client: None,
            is_byok: true,
        }
    }

    fn with_base_url(mut self, base_url: Option<String>) -> Self {
        if let Some(base_url) = base_url
            .map(|value| value.trim().trim_end_matches('/').to_string())
            .filter(|value| !value.is_empty())
        {
            self.chat_completions_url = if base_url.ends_with("/chat/completions") {
                base_url
            } else if base_url.ends_with("/v1") {
                format!("{base_url}/chat/completions")
            } else {
                format!("{base_url}/v1/chat/completions")
            };
        }
        self
    }

    fn with_headers(mut self, organization: Option<String>, project: Option<String>) -> Self {
        self.organization = organization
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.project = project
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self
    }

    pub fn with_billing_context(
        mut self,
        deployment_id: i64,
        thread_id: i64,
        actor_id: i64,
        nats_client: async_nats::Client,
    ) -> Self {
        self.deployment_id = Some(deployment_id);
        self.thread_id = Some(thread_id);
        self.actor_id = Some(actor_id);
        self.nats_client = Some(nats_client);
        self
    }

    async fn track_token_usage(&self, usage: &UsageMetadata) {
        crate::llm::usage::publish_model_usage(
            crate::llm::usage::ModelUsageContext {
                deployment_id: self.deployment_id,
                thread_id: self.thread_id,
                actor_id: self.actor_id,
                model: &self.model,
                is_byok: self.is_byok,
                nats_client: self.nats_client.as_ref(),
                search_queries: &[],
            },
            usage,
        )
        .await;
    }

    pub async fn generate_structured_from_prompt<T>(
        &self,
        prompt: SemanticLlmRequest,
        _cache: Option<PromptCacheRequest>,
    ) -> Result<StructuredGenerationOutput<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let request_body = self.build_request_body(prompt);
        let parsed = self.execute_request(request_body).await?;

        let generated_text = parsed
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .map(Self::message_content_as_text)
            .unwrap_or_default();

        if generated_text.is_empty() {
            return Err(AppError::Internal(format!(
                "No response content from OpenAI API",
            )));
        }

        let value = serde_json::from_str::<T>(&generated_text).map_err(|error| {
            AppError::Internal(format!(
                "Failed to parse OpenAI generated content: {}. Generated text (first 2000 chars): {}",
                error,
                generated_text.chars().take(2000).collect::<String>()
            ))
        })?;

        let usage_metadata = parsed.usage.map(Self::map_usage_metadata);
        if let Some(usage) = &usage_metadata {
            self.track_token_usage(usage).await;
        }
        Ok(StructuredGenerationOutput {
            value,
            usage_metadata,
            cache_state: None,
        })
    }

    pub async fn generate_text_from_prompt(
        &self,
        prompt: SemanticLlmRequest,
    ) -> Result<crate::llm::TextGenerationOutput, AppError> {
        let request_body = self.build_text_request_body(prompt);
        let parsed = self.execute_request(request_body).await?;

        let generated_text = parsed
            .choices
            .first()
            .and_then(|choice| choice.message.content.as_ref())
            .map(Self::message_content_as_text)
            .unwrap_or_default();

        if generated_text.is_empty() {
            return Err(AppError::Internal(
                "No response content from OpenAI API".to_string(),
            ));
        }

        let usage_metadata = parsed.usage.map(Self::map_usage_metadata);
        if let Some(usage) = &usage_metadata {
            self.track_token_usage(usage).await;
        }
        Ok(crate::llm::TextGenerationOutput {
            text: generated_text,
            usage_metadata,
        })
    }

    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
        cache: Option<PromptCacheRequest>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        let request_body = self.build_tool_call_request_body(prompt, tools, cache);
        let parsed = self.execute_request(request_body).await?;
        let message = &parsed
            .choices
            .first()
            .ok_or_else(|| AppError::Internal("OpenAI returned no choices".to_string()))?
            .message;

        // Parse each tool call independently: one unparseable arguments blob
        // (usually a length-truncated call) skips that call instead of failing the
        // whole turn. If nothing parses and there's no text, the loop's empty guard handles it.
        let calls = match message.tool_calls.as_ref() {
            Some(tc) => tc
                .iter()
                .filter_map(
                    |call| match serde_json::from_str::<Value>(&call.function.arguments) {
                        Ok(arguments) => Some(GeneratedToolCall {
                            tool_name: call.function.name.clone(),
                            arguments,
                            signature: None,
                        }),
                        Err(error) => {
                            tracing::warn!(
                                tool = %call.function.name,
                                %error,
                                "skipping OpenAI tool call with unparseable arguments (likely truncated)"
                            );
                            None
                        }
                    },
                )
                .collect::<Vec<_>>(),
            None => Vec::new(),
        };

        let content_text = message
            .content
            .as_ref()
            .map(Self::message_content_as_text)
            .filter(|t| !t.trim().is_empty());

        if calls.is_empty() && content_text.is_none() {
            // Empty turn, not an error — let the loop's empty-response guard handle it.
            tracing::warn!("OpenAI returned no tool calls and no text");
        }

        let finish_reason = parsed
            .choices
            .first()
            .and_then(|choice| choice.finish_reason.clone());

        let usage_metadata = parsed.usage.map(Self::map_usage_metadata);
        if let Some(usage) = &usage_metadata {
            self.track_token_usage(usage).await;
        }
        Ok(ToolCallGenerationOutput {
            calls,
            content_text,
            usage_metadata,
            cache_state: None,
            finish_reason,
        })
    }

    fn build_text_request_body(&self, prompt: SemanticLlmRequest) -> Value {
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        messages.push(self.system_message(&prompt.system_prompt));
        messages.extend(
            prompt
                .messages
                .into_iter()
                .flat_map(Self::semantic_message_to_openai),
        );

        let mut body = serde_json::Map::new();
        body.insert("model".to_string(), json!(self.model));
        body.insert("messages".to_string(), Value::Array(messages));
        body.insert("stream".to_string(), json!(false));
        if let Some(temperature) = prompt.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(max_output_tokens) = prompt.max_output_tokens {
            body.insert(
                "max_completion_tokens".to_string(),
                json!(max_output_tokens),
            );
        }
        if let Some(reasoning_effort) = prompt.reasoning_effort {
            body.insert("reasoning_effort".to_string(), json!(reasoning_effort));
        }
        Value::Object(body)
    }

    fn build_request_body(&self, prompt: SemanticLlmRequest) -> Value {
        let response_json_schema = normalize_openai_response_schema(prompt.response_json_schema);
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        messages.push(self.system_message(&prompt.system_prompt));
        messages.extend(
            prompt
                .messages
                .into_iter()
                .flat_map(Self::semantic_message_to_openai),
        );

        let mut body = serde_json::Map::new();
        body.insert("model".to_string(), json!(self.model));
        body.insert("messages".to_string(), Value::Array(messages));
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
            body.insert(
                "max_completion_tokens".to_string(),
                json!(max_output_tokens),
            );
        }
        if let Some(reasoning_effort) = prompt.reasoning_effort {
            body.insert("reasoning_effort".to_string(), json!(reasoning_effort));
        }
        Value::Object(body)
    }

    fn build_tool_call_request_body(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
        cache: Option<PromptCacheRequest>,
    ) -> Value {
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        messages.push(self.system_message(&prompt.system_prompt));
        messages.extend(
            prompt
                .messages
                .into_iter()
                .flat_map(Self::semantic_message_to_openai),
        );

        let tool_values = tools
            .into_iter()
            .map(|tool| {
                let strict = !schema_has_free_form_object(&tool.input_schema);
                let parameters = normalize_openai_tool_schema(tool.input_schema, strict);
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name,
                        "description": tool.description,
                        "strict": strict,
                        "parameters": parameters,
                    }
                })
            })
            .collect::<Vec<_>>();

        let mut body = serde_json::Map::new();
        body.insert("model".to_string(), json!(self.model));
        body.insert("messages".to_string(), Value::Array(messages));
        body.insert("tools".to_string(), Value::Array(tool_values));
        // Honor forced tool selection: a single forced name pins that function,
        // multiple forces "required" (must call one of the provided tools),
        // otherwise the model is free to choose.
        let tool_choice = match prompt.forced_tool_names.as_deref() {
            Some([name]) => json!({"type": "function", "function": {"name": name}}),
            Some(names) if !names.is_empty() => json!("required"),
            _ => json!("auto"),
        };
        body.insert("tool_choice".to_string(), tool_choice);
        body.insert("stream".to_string(), json!(false));
        if let Some(cache_request) = cache.as_ref() {
            body.insert(
                "prompt_cache_key".to_string(),
                json!(cache_request.cache_key),
            );
        }
        if let Some(temperature) = prompt.temperature {
            body.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(max_output_tokens) = prompt.max_output_tokens {
            body.insert(
                "max_completion_tokens".to_string(),
                json!(max_output_tokens),
            );
        }
        if let Some(reasoning_effort) = prompt.reasoning_effort {
            body.insert("reasoning_effort".to_string(), json!(reasoning_effort));
        }
        Value::Object(body)
    }

    async fn execute_request(&self, request_body: Value) -> Result<OpenAiResponse, AppError> {
        let mut attempt = 0u32;
        let response_text = loop {
            let request = self
                .client
                .post(&self.chat_completions_url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS));
            let request = if let Some(organization) = self.organization.as_deref() {
                request.header("OpenAI-Organization", organization)
            } else {
                request
            };
            let request = if let Some(project) = self.project.as_deref() {
                request.header("OpenAI-Project", project)
            } else {
                request
            };

            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    let should_retry =
                        error.is_timeout() || error.is_connect() || error.is_request();
                    attempt += 1;
                    if should_retry && attempt < MAX_RETRIES {
                        tokio::time::sleep(Self::calculate_backoff_delay(attempt)).await;
                        continue;
                    }
                    return Err(AppError::Internal(format!(
                        "OpenAI request failed: {}",
                        error
                    )));
                }
            };

            let status = response.status();
            let response_text = response.text().await.map_err(|error| {
                AppError::Internal(format!("Failed to read OpenAI response body: {}", error))
            })?;

            if status.is_success() && Self::parse_error_envelope(&response_text).is_none() {
                break response_text;
            }

            let should_retry = Self::should_retry_response(status.as_u16(), &response_text);
            attempt += 1;
            if should_retry && attempt < MAX_RETRIES {
                tokio::time::sleep(Self::calculate_backoff_delay(attempt)).await;
                continue;
            }

            return Err(AppError::Internal(format!(
                "OpenAI request failed with status {}: {}",
                status, response_text
            )));
        };

        serde_json::from_str(&response_text).map_err(|error| {
            AppError::Internal(format!(
                "Failed to parse OpenAI response JSON: {}. Raw body (first 2000 chars): {}",
                error,
                response_text.chars().take(2000).collect::<String>()
            ))
        })
    }

    fn parse_error_envelope(response_text: &str) -> Option<OpenAiErrorEnvelope> {
        serde_json::from_str::<OpenAiErrorEnvelope>(response_text).ok()
    }

    fn error_code_as_u64(code: &Value) -> Option<u64> {
        match code {
            Value::Number(number) => number.as_u64(),
            Value::String(text) => text.parse::<u64>().ok(),
            _ => None,
        }
    }

    fn should_retry_response(status_code: u16, response_text: &str) -> bool {
        if matches!(status_code, 408 | 409 | 429 | 500 | 502 | 503 | 504) {
            return true;
        }

        Self::parse_error_envelope(response_text)
            .and_then(|envelope| envelope.error.code)
            .as_ref()
            .and_then(Self::error_code_as_u64)
            .map(|code| matches!(code, 408 | 409 | 429 | 500 | 502 | 503 | 504 | 524))
            .unwrap_or(false)
    }

    // RETRY POLICY (explicit product requirement):
    // Fixed random delay between retry attempts — uniformly in [2000ms, 4000ms].
    // No exponential backoff, no Retry-After header honoring; the fixed window
    // is the contract. Matches `MAX_RETRIES: u32 = 15` at module scope.
    fn calculate_backoff_delay(_attempt: u32) -> Duration {
        let jitter_ms = 2000 + (rand::random::<f64>() * 2000.0) as u64;
        Duration::from_millis(jitter_ms)
    }

    fn system_message(&self, system_prompt: &str) -> Value {
        json!({
            "role": "system",
            "content": system_prompt,
        })
    }

    fn semantic_message_to_openai(message: SemanticLlmMessage) -> Vec<Value> {
        let role = match message.role.as_str() {
            "model" | "assistant" => "assistant",
            "system" => "system",
            "developer" => "developer",
            _ => "user",
        };

        let tool_results = message
            .content_blocks
            .iter()
            .filter_map(|block| match block {
                SemanticLlmContentBlock::ToolResult {
                    call_id, output, ..
                } => Some(json!({
                    "role": "tool",
                    "tool_call_id": call_id,
                    "content": serde_json::to_string(output).unwrap_or_default(),
                })),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !tool_results.is_empty() {
            return tool_results;
        }

        let tool_calls = message
            .content_blocks
            .iter()
            .filter_map(|block| match block {
                SemanticLlmContentBlock::ToolCall {
                    id, name, args, ..
                } => Some(json!({
                    "id": id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string()),
                    },
                })),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !tool_calls.is_empty() {
            let text = message
                .content_blocks
                .iter()
                .filter_map(|block| match block {
                    SemanticLlmContentBlock::Text { text } => Some(text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            let content = if text.trim().is_empty() {
                Value::Null
            } else {
                Value::String(text)
            };
            return vec![json!({
                "role": "assistant",
                "content": content,
                "tool_calls": tool_calls,
            })];
        }

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
                SemanticLlmContentBlock::ToolCall { .. }
                | SemanticLlmContentBlock::ToolResult { .. } => {
                    json!({ "type": "text", "text": "" })
                }
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

        vec![json!({
            "role": role,
            "content": content,
        })]
    }

    fn message_content_as_text(content: &OpenAiMessageContent) -> String {
        match content {
            OpenAiMessageContent::Text(text) => text.clone(),
            OpenAiMessageContent::Parts(parts) => parts
                .iter()
                .filter(|part| part.part_type.as_deref() == Some("text"))
                .filter_map(|part| part.text.as_ref())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    fn map_usage_metadata(usage: OpenAiUsage) -> UsageMetadata {
        UsageMetadata {
            prompt_token_count: usage.prompt_tokens,
            cached_content_token_count: usage
                .prompt_tokens_details
                .as_ref()
                .and_then(|details| details.cached_tokens),
            candidates_token_count: usage.completion_tokens,
            total_token_count: usage.total_tokens,
            thoughts_token_count: usage
                .completion_tokens_details
                .as_ref()
                .and_then(|details| details.reasoning_tokens),
            tool_use_prompt_token_count: None,
            cache_write_token_count: None,
            prompt_tokens_details: None,
            cache_tokens_details: None,
            candidates_tokens_details: None,
            tool_use_prompt_tokens_details: None,
        }
    }
}

#[async_trait::async_trait]
impl crate::llm::LlmProvider for OpenAiClient {
    fn provider_label(&self) -> &'static str {
        "openai"
    }

    async fn generate_structured(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<StructuredGenerationOutput<Value>, AppError> {
        OpenAiClient::generate_structured_from_prompt::<Value>(self, prompt, cache).await
    }

    async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
        cache: Option<PromptCacheRequest>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        OpenAiClient::generate_tool_calls(self, prompt, tools, cache).await
    }

    async fn generate_text(
        &self,
        prompt: SemanticLlmRequest,
    ) -> Result<crate::llm::TextGenerationOutput, AppError> {
        OpenAiClient::generate_text_from_prompt(self, prompt).await
    }
}
