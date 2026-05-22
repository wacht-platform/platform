mod billing;
mod cache;
mod types;

use common::ResultExt;
use crate::json_schema::normalize_gemini_function_schema;
use common::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

use crate::llm::{
    GeneratedToolCall, NativeToolDefinition, PromptCacheRequest, SemanticLlmRequest,
    ToolCallGenerationOutput,
};

pub use types::{
    ExplicitCacheRequest, GeminiResponse, StructuredContentOutput, UsageMetadata,
    GEMINI_STRUCTURED_OUTPUT_TRUNCATED_MARKER,
};

const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const REQUEST_TIMEOUT_SECS: u64 = 240;

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
    deployment_id: Option<i64>,
    thread_id: Option<i64>,
    actor_id: Option<i64>,
    redis_client: Option<redis::Client>,
    nats_client: Option<async_nats::Client>,
    is_byok: bool,
}

impl GeminiClient {
    fn parse_error_envelope(response_text: &str) -> Option<(u64, String)> {
        let value = serde_json::from_str::<Value>(response_text).ok()?;
        let error = value.get("error")?;
        let code = error.get("code")?.as_u64()?;
        let message = error
            .get("message")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        Some((code, message))
    }

    fn should_retry_response(status_code: u16, response_text: &str) -> bool {
        if matches!(status_code, 408 | 409 | 429 | 500 | 502 | 503 | 504) {
            return true;
        }

        Self::parse_error_envelope(response_text)
            .map(|(code, _)| matches!(code, 408 | 409 | 429 | 500 | 502 | 503 | 504))
            .unwrap_or(false)
    }

    pub fn new_byok(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
            deployment_id: None,
            thread_id: None,
            actor_id: None,
            redis_client: None,
            nats_client: None,
            is_byok: true,
        }
    }

    pub fn with_billing(mut self, deployment_id: i64, redis_client: redis::Client) -> Self {
        self.deployment_id = Some(deployment_id);
        self.redis_client = Some(redis_client);
        self
    }

    pub fn with_nats(mut self, nats_client: async_nats::Client) -> Self {
        self.nats_client = Some(nats_client);
        self
    }

    pub fn with_thread(mut self, thread_id: i64) -> Self {
        self.thread_id = Some(thread_id);
        self
    }

    pub fn with_actor(mut self, actor_id: i64) -> Self {
        self.actor_id = Some(actor_id);
        self
    }

    pub fn from_api_key(
        deployment_api_key: Option<String>,
        model: &str,
        deployment_id: i64,
        thread_id: i64,
        actor_id: i64,
        redis_client: redis::Client,
        nats_client: async_nats::Client,
    ) -> Result<Self, AppError> {
        if let Some(api_key) = deployment_api_key {
            return Ok(Self::new_byok(api_key, model.to_string())
                .with_billing(deployment_id, redis_client)
                .with_nats(nats_client)
                .with_thread(thread_id)
                .with_actor(actor_id));
        }
        Err(AppError::BadRequest(
            "Gemini API key is not configured for this deployment".to_string(),
        ))
    }

    pub async fn generate_structured_content_with_usage_and_cache<T>(
        &self,
        request_body: String,
        cache_request: Option<ExplicitCacheRequest>,
    ) -> Result<StructuredContentOutput<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);
        let prepared_request = self
            .prepare_generate_request_body(request_body, cache_request.as_ref())
            .await;
        let request_body = prepared_request.request_body;
        let cache_plan = prepared_request.cache_plan;
        let parsed = self
            .execute_generate_content_request(&url, &request_body)
            .await?;

        let generated_text = Self::response_text(&parsed);
        if generated_text.is_empty() {
            return Err(AppError::Internal(
                "No response content from Gemini API".to_string(),
            ));
        }

        let parsed_response = serde_json::from_str::<T>(&generated_text).map_err(|e| {
            let preview = generated_text.chars().take(2000).collect::<String>();
            if e.is_eof() {
                AppError::Internal(format!(
                    "{}: Failed to parse Gemini generated content: {}. Generated text (first 2000 chars): {}",
                    GEMINI_STRUCTURED_OUTPUT_TRUNCATED_MARKER,
                    e,
                    preview
                ))
            } else {
                AppError::Internal(format!(
                    "Failed to parse Gemini generated content: {}. Generated text (first 2000 chars): {}",
                    e, preview
                ))
            }
        })?;

        if let Some(usage) = parsed.usage_metadata.as_ref() {
            self.track_token_usage(usage, &parsed).await;
        }

        let cache_state = if let (Some(cache_request), Some(cache_plan)) =
            (cache_request.as_ref(), cache_plan.as_ref())
        {
            if cache_request.reuse_only {
                None
            } else {
                self.refresh_explicit_cache(cache_request, cache_plan)
                    .await?
            }
        } else {
            None
        };

        Ok(StructuredContentOutput {
            value: parsed_response,
            usage_metadata: parsed.usage_metadata,
            cache_state,
        })
    }

    pub async fn generate_text(
        &self,
        request_body: String,
    ) -> Result<crate::llm::TextGenerationOutput, AppError> {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);
        let parsed = self
            .execute_generate_content_request(&url, &request_body)
            .await?;
        let text = Self::response_text(&parsed);
        if text.is_empty() {
            return Err(AppError::Internal(
                "No response content from Gemini API".to_string(),
            ));
        }
        if let Some(usage) = parsed.usage_metadata.as_ref() {
            self.track_token_usage(usage, &parsed).await;
        }
        Ok(crate::llm::TextGenerationOutput {
            text,
            usage_metadata: parsed.usage_metadata,
        })
    }

    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
        cache: Option<PromptCacheRequest>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);
        let request_body = self.build_tool_call_request_body(prompt, tools)?;

        let cache_request: Option<ExplicitCacheRequest> = cache.map(Into::into);
        let prepared = self
            .prepare_generate_request_body(request_body, cache_request.as_ref())
            .await;
        let request_body = prepared.request_body;
        let cache_plan = prepared.cache_plan;

        let parsed = self
            .execute_generate_content_request(&url, &request_body)
            .await?;

        if let Some(usage) = parsed.usage_metadata.as_ref() {
            self.track_token_usage(usage, &parsed).await;
        }

        let calls = parsed
            .candidates
            .iter()
            .flat_map(|candidate| candidate.content.parts.iter())
            .filter_map(|part| part.function_call.as_ref())
            .map(|call| GeneratedToolCall {
                tool_name: call.name.clone(),
                arguments: call.args.clone(),
            })
            .collect::<Vec<_>>();

        let text = Self::response_text(&parsed);
        let content_text = if text.trim().is_empty() {
            None
        } else {
            Some(text)
        };

        if calls.is_empty() && content_text.is_none() {
            return Err(AppError::Internal(
                "Gemini returned no function calls and no text".to_string(),
            ));
        }

        let cache_state = if let (Some(cache_request), Some(cache_plan)) =
            (cache_request.as_ref(), cache_plan.as_ref())
        {
            if cache_request.reuse_only {
                None
            } else {
                self.refresh_explicit_cache(cache_request, cache_plan)
                    .await?
            }
        } else {
            None
        };

        Ok(ToolCallGenerationOutput {
            calls,
            content_text,
            usage_metadata: parsed.usage_metadata,
            cache_state,
        })
    }

    fn response_text(response: &GeminiResponse) -> String {
        response
            .candidates
            .iter()
            .flat_map(|candidate| candidate.content.parts.iter())
            .filter_map(|part| part.text.as_deref())
            .collect::<String>()
    }

    fn build_tool_call_request_body(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
    ) -> Result<String, AppError> {
        let contents = prompt
            .messages
            .iter()
            .map(|message| {
                let parts = message
                    .content_blocks
                    .iter()
                    .map(|block| match block {
                        crate::llm::SemanticLlmContentBlock::Text { text } => {
                            json!({ "text": text })
                        }
                        crate::llm::SemanticLlmContentBlock::InlineData { mime_type, data } => {
                            json!({ "inline_data": { "mime_type": mime_type, "data": data } })
                        }
                    })
                    .collect::<Vec<_>>();
                json!({ "role": message.role, "parts": parts })
            })
            .collect::<Vec<_>>();

        let function_declarations = tools
            .into_iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": normalize_gemini_function_schema(tool.input_schema),
                })
            })
            .collect::<Vec<_>>();

        let mut generation_config = serde_json::Map::new();
        if let Some(temperature) = prompt.temperature {
            generation_config.insert("temperature".to_string(), json!(temperature));
        }
        if let Some(max_output_tokens) = prompt.max_output_tokens {
            generation_config.insert("maxOutputTokens".to_string(), json!(max_output_tokens));
        }
        if let Some(ref reasoning_effort) = prompt.reasoning_effort {
            generation_config.insert(
                "thinkingConfig".to_string(),
                thinking_config_for(reasoning_effort),
            );
        }

        let mut body = serde_json::Map::new();
        body.insert(
            "system_instruction".to_string(),
            json!({ "parts": [{ "text": prompt.system_prompt }] }),
        );
        body.insert("contents".to_string(), json!(contents));
        body.insert(
            "tools".to_string(),
            json!([{ "functionDeclarations": function_declarations }]),
        );
        let tool_config = if let Some(forced) = prompt.forced_tool_names.as_ref() {
            json!({
                "functionCallingConfig": {
                    "mode": "ANY",
                    "allowedFunctionNames": forced,
                }
            })
        } else {
            // VALIDATED: model picks either a function call or natural language
            // (same flexibility as AUTO, so the agent can still emit a pure-text
            // terminal response). Gemini additionally enforces function-call
            // schema adherence, which reduces malformed-args parse failures we
            // were occasionally hitting on AUTO.
            json!({ "functionCallingConfig": { "mode": "VALIDATED" } })
        };
        body.insert("toolConfig".to_string(), tool_config);
        body.insert("safetySettings".to_string(), gemini_safety_settings());
        if !generation_config.is_empty() {
            body.insert(
                "generationConfig".to_string(),
                serde_json::Value::Object(generation_config),
            );
        }

        serde_json::to_string(&serde_json::Value::Object(body)).map_err(|e| {
            AppError::Internal(format!("Failed to serialize Gemini tool-call request: {e}"))
        })
    }

    async fn execute_generate_content_request(
        &self,
        url: &str,
        request_body: &str,
    ) -> Result<GeminiResponse, AppError> {
        let mut attempt = 0u32;
        // RETRY POLICY (explicit product requirement):
        // - Up to 15 attempts on retryable failures (timeouts, 5xx, 429, transient parse).
        // - Fixed random delay between attempts, uniformly in [2s, 4s].
        // - No exponential backoff. Do not honor Retry-After; the fixed window is the contract.
        const MAX_RETRIES: u32 = 15;
        let mut current_body = request_body.to_string();
        let mut safety_retry_used = false;

        let last_error = loop {
            let response = self
                .client
                .post(url)
                .header("x-goog-api-key", &self.api_key)
                .header("Content-Type", "application/json")
                .body(current_body.clone())
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let response_text = match resp.text().await {
                        Ok(body) => body,
                        Err(e) => {
                            let error = format!("Failed to read Gemini response body: {e}");
                            attempt += 1;
                            if attempt < MAX_RETRIES {
                                tokio::time::sleep(Self::calculate_backoff_delay(attempt)).await;
                                continue;
                            }
                            break error;
                        }
                    };
                    if !status.is_success() || Self::parse_error_envelope(&response_text).is_some()
                    {
                        let error = format!(
                            "Gemini request failed with status {}: {}",
                            status,
                            response_text.chars().take(500).collect::<String>()
                        );

                        attempt += 1;
                        if Self::should_retry_response(status.as_u16(), &response_text)
                            && attempt < MAX_RETRIES
                        {
                            tokio::time::sleep(Self::calculate_backoff_delay(attempt)).await;
                            continue;
                        }
                        break error;
                    }

                    let parsed: GeminiResponse =
                        serde_json::from_str(&response_text).map_err(|e| {
                            AppError::Internal(format!(
                                "Failed to parse Gemini response JSON: {}. Raw body (first 1000 chars): {}",
                                e,
                                response_text.chars().take(1000).collect::<String>()
                            ))
                        })?;
                    if parsed.candidates.is_empty() {
                        let block = parsed
                            .prompt_feedback
                            .as_ref()
                            .and_then(|f| f.block_reason.as_deref())
                            .unwrap_or("unknown")
                            .to_string();
                        if !safety_retry_used {
                            safety_retry_used = true;
                            current_body = inject_safety_steering(&current_body, &block)?;
                            continue;
                        }
                        return Err(AppError::Internal(format!(
                            "Gemini returned no candidates after safety re-prompt (block_reason={block}). Raw body (first 500 chars): {}",
                            response_text.chars().take(500).collect::<String>()
                        )));
                    }
                    return Ok(parsed);
                }
                Err(e) => {
                    let error_kind = if e.is_timeout() {
                        "timeout"
                    } else if e.is_connect() {
                        "connect"
                    } else if e.is_request() {
                        "request"
                    } else if e.is_body() {
                        "body"
                    } else if e.is_decode() {
                        "decode"
                    } else {
                        "other"
                    };
                    let error = format!("Request failed ({error_kind}): {e}");

                    attempt += 1;
                    if attempt < MAX_RETRIES {
                        tokio::time::sleep(Self::calculate_backoff_delay(attempt)).await;
                        continue;
                    }
                    break error;
                }
            }
        };

        Err(AppError::Internal(format!(
            "Failed after {} attempts: {}",
            attempt, last_error
        )))
    }

    // RETRY POLICY (explicit product requirement):
    // Fixed random delay between retry attempts — uniformly in [2000ms, 4000ms].
    // No exponential backoff, no Retry-After header honoring; the fixed window is
    // the contract. See `execute_generate_content_request` for the matching
    // `MAX_RETRIES: u32 = 15`.
    fn calculate_backoff_delay(_attempt: u32) -> Duration {
        let jitter_ms = 2000 + (rand::random::<f64>() * 2000.0) as u64;
        let final_delay = jitter_ms;

        Duration::from_millis(final_delay)
    }
}

fn serialize_gemini_structured_request(prompt: &SemanticLlmRequest) -> Result<String, AppError> {
    serialize_gemini_request_inner(prompt, true)
}

fn serialize_gemini_text_request(prompt: &SemanticLlmRequest) -> Result<String, AppError> {
    serialize_gemini_request_inner(prompt, false)
}

fn serialize_gemini_request_inner(
    prompt: &SemanticLlmRequest,
    include_response_schema: bool,
) -> Result<String, AppError> {
    use crate::llm::SemanticLlmContentBlock;
    let contents = prompt
        .messages
        .iter()
        .map(|message| {
            let parts = message
                .content_blocks
                .iter()
                .map(|block| match block {
                    SemanticLlmContentBlock::Text { text } => json!({ "text": text }),
                    SemanticLlmContentBlock::InlineData { mime_type, data } => {
                        json!({ "inline_data": { "mime_type": mime_type, "data": data } })
                    }
                })
                .collect::<Vec<_>>();
            json!({ "role": message.role, "parts": parts })
        })
        .collect::<Vec<_>>();

    let mut generation_config = serde_json::Map::new();
    if include_response_schema {
        generation_config.insert("responseMimeType".to_string(), json!("application/json"));
        generation_config.insert(
            "responseSchema".to_string(),
            prompt.response_json_schema.clone(),
        );
    }
    if let Some(temperature) = prompt.temperature {
        generation_config.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(max_output_tokens) = prompt.max_output_tokens {
        generation_config.insert("maxOutputTokens".to_string(), json!(max_output_tokens));
    }
    if let Some(reasoning_effort) = prompt.reasoning_effort.as_ref() {
        generation_config.insert(
            "thinkingConfig".to_string(),
            thinking_config_for(reasoning_effort),
        );
    }

    serde_json::to_string(&json!({
        "system_instruction": { "parts": [{ "text": prompt.system_prompt }] },
        "contents": contents,
        "generationConfig": generation_config,
        "safetySettings": gemini_safety_settings(),
    }))
    .map_err_internal("Failed to serialize LLM request")
}

fn gemini_safety_settings() -> Value {
    json!([
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": "BLOCK_NONE" },
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_NONE" },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": "BLOCK_NONE" },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": "BLOCK_NONE" },
        { "category": "HARM_CATEGORY_CIVIC_INTEGRITY", "threshold": "BLOCK_NONE" },
    ])
}

fn inject_safety_steering(body: &str, block_reason: &str) -> Result<String, AppError> {
    let steering = format!(
        "\n\nIMPORTANT — the previous attempt was blocked by Gemini's content safety filters \
         (block_reason={block_reason}). Re-generate a response that completes the task while \
         staying within safety guidelines: rephrase, sanitize, summarize, or describe at a higher \
         level of abstraction as needed. Do not refuse the task — produce the best safe version of \
         the requested output."
    );
    let mut value: Value = serde_json::from_str(body).map_err(|e| {
        AppError::Internal(format!("Failed to parse Gemini request for re-prompt: {e}"))
    })?;
    let obj = value.as_object_mut().ok_or_else(|| {
        AppError::Internal("Gemini request body is not a JSON object".to_string())
    })?;
    let sys = obj
        .entry("system_instruction".to_string())
        .or_insert_with(|| json!({ "parts": [{ "text": "" }] }));
    let parts = sys
        .get_mut("parts")
        .and_then(|p| p.as_array_mut())
        .ok_or_else(|| AppError::Internal("system_instruction.parts missing".to_string()))?;
    if parts.is_empty() {
        parts.push(json!({ "text": steering.trim_start() }));
    } else {
        let first = parts[0].as_object_mut().ok_or_else(|| {
            AppError::Internal("system_instruction.parts[0] not an object".to_string())
        })?;
        let existing = first
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        first.insert(
            "text".to_string(),
            Value::String(format!("{existing}{steering}")),
        );
    }
    serde_json::to_string(&value)
        .map_err_internal("Failed to re-serialize Gemini request")
}

// Gemini 2.5 uses thinkingBudget (integer); Gemini 3+ uses thinkingLevel (string).
// Detect by whether the value parses as an integer.
fn thinking_config_for(reasoning_effort: &str) -> Value {
    if let Ok(budget) = reasoning_effort.parse::<i64>() {
        json!({ "thinkingBudget": budget })
    } else {
        json!({ "thinkingLevel": reasoning_effort })
    }
}

#[async_trait::async_trait]
impl crate::llm::LlmProvider for GeminiClient {
    fn provider_label(&self) -> &'static str {
        "gemini"
    }

    async fn generate_structured(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<crate::llm::StructuredGenerationOutput<Value>, AppError> {
        let body = serialize_gemini_structured_request(&prompt)?;
        let output = self
            .generate_structured_content_with_usage_and_cache::<Value>(body, cache.map(Into::into))
            .await?;
        Ok(crate::llm::StructuredGenerationOutput {
            value: output.value,
            usage_metadata: output.usage_metadata,
            cache_state: output.cache_state,
        })
    }

    async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
        cache: Option<PromptCacheRequest>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        GeminiClient::generate_tool_calls(self, prompt, tools, cache).await
    }

    async fn generate_text(
        &self,
        prompt: SemanticLlmRequest,
    ) -> Result<crate::llm::TextGenerationOutput, AppError> {
        let body = serialize_gemini_text_request(&prompt)?;
        GeminiClient::generate_text(self, body).await
    }
}
