use std::time::Duration;

use common::error::AppError;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{
    json_schema::{normalize_openai_response_schema, normalize_openai_tool_schema},
    llm::{
        GeneratedToolCall, NativeToolDefinition, PromptCacheRequest, SemanticLlmContentBlock,
        SemanticLlmMessage, SemanticLlmRequest, StructuredGenerationOutput,
        ToolCallGenerationOutput, UsageMetadata,
    },
};

const OPENAI_CHAT_COMPLETIONS_URL: &str = "https://api.openai.com/v1/chat/completions";
const REQUEST_TIMEOUT_SECS: u64 = 240;
const MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct OpenAiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
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

    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
        }
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

        Ok(StructuredGenerationOutput {
            value,
            usage_metadata: parsed.usage.map(Self::map_usage_metadata),
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

        Ok(crate::llm::TextGenerationOutput {
            text: generated_text,
            usage_metadata: parsed.usage.map(Self::map_usage_metadata),
        })
    }

    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        let request_body = self.build_tool_call_request_body(prompt, tools);
        let parsed = self.execute_request(request_body).await?;
        let message = &parsed
            .choices
            .first()
            .ok_or_else(|| AppError::Internal("OpenAI returned no choices".to_string()))?
            .message;

        let calls = match message.tool_calls.as_ref() {
            Some(tc) => tc
                .iter()
                .map(|call| {
                    let arguments = serde_json::from_str::<Value>(&call.function.arguments)
                        .map_err(|error| {
                            AppError::Internal(format!(
                                "Failed to parse OpenAI tool arguments for {}: {}",
                                call.function.name, error
                            ))
                        })?;
                    Ok(GeneratedToolCall {
                        tool_name: call.function.name.clone(),
                        arguments,
                    })
                })
                .collect::<Result<Vec<_>, AppError>>()?,
            None => Vec::new(),
        };

        let content_text = message
            .content
            .as_ref()
            .map(Self::message_content_as_text)
            .filter(|t| !t.trim().is_empty());

        if calls.is_empty() && content_text.is_none() {
            return Err(AppError::Internal(
                "OpenAI returned no tool calls and no text".to_string(),
            ));
        }

        Ok(ToolCallGenerationOutput {
            calls,
            content_text,
            usage_metadata: parsed.usage.map(Self::map_usage_metadata),
        })
    }

    fn build_text_request_body(&self, prompt: SemanticLlmRequest) -> Value {
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        messages.push(self.system_message(&prompt.system_prompt));
        messages.extend(
            prompt
                .messages
                .into_iter()
                .map(Self::semantic_message_to_openai),
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
                .map(Self::semantic_message_to_openai),
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
    ) -> Value {
        let mut messages = Vec::with_capacity(prompt.messages.len() + 1);
        messages.push(self.system_message(&prompt.system_prompt));
        messages.extend(
            prompt
                .messages
                .into_iter()
                .map(Self::semantic_message_to_openai),
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
        // AUTO mode: model may emit tool calls, text, or both. Text-only response is
        // the terminal signal in the unified ReAct loop.
        body.insert("tool_choice".to_string(), json!("auto"));
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

    async fn execute_request(&self, request_body: Value) -> Result<OpenAiResponse, AppError> {
        let mut attempt = 0u32;
        let response_text = loop {
            let request = self
                .client
                .post(OPENAI_CHAT_COMPLETIONS_URL)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&request_body)
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS));

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
            let retry_delay = Self::retry_delay_from_headers(response.headers());
            let response_text = response.text().await.map_err(|error| {
                AppError::Internal(format!("Failed to read OpenAI response body: {}", error))
            })?;

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

    fn retry_delay_from_headers(headers: &reqwest::header::HeaderMap) -> Option<Duration> {
        let retry_after = headers.get(reqwest::header::RETRY_AFTER)?;
        let retry_after = retry_after.to_str().ok()?.trim();
        let secs = retry_after.parse::<f64>().ok()?;
        Some(Duration::from_secs_f64(secs.max(0.5)))
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

    fn semantic_message_to_openai(message: SemanticLlmMessage) -> Value {
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
            prompt_tokens_details: None,
            cache_tokens_details: None,
            candidates_tokens_details: None,
            tool_use_prompt_tokens_details: None,
        }
    }
}
