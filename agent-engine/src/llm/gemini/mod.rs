mod billing;
mod cache;
mod types;

use crate::json_schema::normalize_gemini_function_schema;
use common::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tracing::info;

use crate::llm::{
    GeneratedToolCall, NativeToolDefinition, SemanticLlmRequest, ToolCallGenerationOutput,
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
            .map(|(code, _)| matches!(code, 408 | 409 | 429 | 500 | 502 | 503 | 504))
            .unwrap_or(false)
    }

    fn sanitize_response_for_logging(response_text: &str) -> String {
        fn strip_thought_signature(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(map) => {
                    map.remove("thoughtSignature");
                    for child in map.values_mut() {
                        strip_thought_signature(child);
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        strip_thought_signature(item);
                    }
                }
                _ => {}
            }
        }

        match serde_json::from_str::<serde_json::Value>(response_text) {
            Ok(mut value) => {
                strip_thought_signature(&mut value);
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| response_text.to_string())
            }
            Err(_) => response_text.to_string(),
        }
    }

    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
            deployment_id: None,
            thread_id: None,
            redis_client: None,
            nats_client: None,
            is_byok: false,
        }
    }

    pub fn new_byok(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
            deployment_id: None,
            thread_id: None,
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

    pub fn with_byok(mut self, is_byok: bool) -> Self {
        self.is_byok = is_byok;
        self
    }

    pub fn with_thread(mut self, thread_id: i64) -> Self {
        self.thread_id = Some(thread_id);
        self
    }

    pub fn from_api_key(
        deployment_api_key: Option<String>,
        model: &str,
        deployment_id: i64,
        thread_id: i64,
        redis_client: redis::Client,
        nats_client: async_nats::Client,
    ) -> Result<Self, AppError> {
        if let Some(api_key) = deployment_api_key {
            return Ok(Self::new_byok(api_key, model.to_string())
                .with_billing(deployment_id, redis_client)
                .with_nats(nats_client)
                .with_thread(thread_id));
        }
        Err(AppError::BadRequest(
            "Gemini API key is not configured for this deployment".to_string(),
        ))
    }

    pub async fn generate_structured_content<T>(&self, request_body: String) -> Result<T, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let output = self
            .generate_structured_content_with_usage::<T>(request_body)
            .await?;
        Ok(output.value)
    }

    pub async fn generate_structured_content_with_usage<T>(
        &self,
        request_body: String,
    ) -> Result<StructuredContentOutput<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        self.generate_structured_content_with_usage_and_cache::<T>(request_body, None)
            .await
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

    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);
        let request_body = self.build_tool_call_request_body(prompt, tools)?;
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

        if calls.is_empty() {
            return Err(AppError::Internal(
                "Gemini returned no function calls".to_string(),
            ));
        }

        Ok(ToolCallGenerationOutput {
            calls,
            usage_metadata: parsed.usage_metadata,
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
        let contents = serde_json::to_value(&json!({
            "system_instruction": {
                "parts": [{ "text": prompt.system_prompt }]
            },
            "contents": prompt
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
                    json!({
                        "role": message.role,
                        "parts": parts,
                    })
                })
                .collect::<Vec<_>>(),
            "tools": [{
                "functionDeclarations": tools
                    .into_iter()
                    .map(|tool| json!({
                        "name": tool.name,
                        "description": tool.description,
                        "parameters": normalize_gemini_function_schema(tool.input_schema),
                    }))
                    .collect::<Vec<_>>()
            }],
            "toolConfig": {
                "functionCallingConfig": {
                    "mode": "ANY"
                }
            }
        }))
        .map_err(|e| {
            AppError::Internal(format!("Failed to build Gemini tool-call request: {e}"))
        })?;

        serde_json::to_string(&contents).map_err(|e| {
            AppError::Internal(format!("Failed to serialize Gemini tool-call request: {e}"))
        })
    }

    async fn execute_generate_content_request(
        &self,
        url: &str,
        request_body: &str,
    ) -> Result<GeminiResponse, AppError> {
        let mut attempt = 0u32;
        const MAX_RETRIES: u32 = 3;
        info!(
            "{}",
            json!({
                "event": "gemini_generate_request",
                "model": self.model,
                "url": url,
                "request": serde_json::from_str::<serde_json::Value>(request_body)
                    .unwrap_or_else(|_| json!({ "raw": request_body })),
            })
            .to_string()
        );

        let last_error = loop {
            let response = self
                .client
                .post(url)
                .header("x-goog-api-key", &self.api_key)
                .header("Content-Type", "application/json")
                .body(request_body.to_string())
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let status = resp.status();
                    let retry_delay = Self::retry_delay_from_headers(resp.headers());
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
                    info!(
                        "{}",
                        json!({
                            "event": "gemini_generate_response",
                            "model": self.model,
                            "url": url,
                            "status": status.as_u16(),
                            "ok": status.is_success(),
                            "response": Self::sanitize_response_for_logging(&response_text),
                        })
                        .to_string()
                    );
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
                            tokio::time::sleep(
                                retry_delay
                                    .unwrap_or_else(|| Self::calculate_backoff_delay(attempt)),
                            )
                            .await;
                            continue;
                        }
                        break error;
                    }

                    return serde_json::from_str(&response_text).map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to parse Gemini response JSON: {}. Raw body (first 1000 chars): {}",
                            e,
                            response_text.chars().take(1000).collect::<String>()
                        ))
                    });
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

    fn calculate_backoff_delay(attempt: u32) -> Duration {
        const INITIAL_DELAY_MS: u64 = 200;
        const MAX_DELAY_MS: u64 = 5000;
        const MULTIPLIER: f64 = 2.0;

        let base_delay =
            INITIAL_DELAY_MS as f64 * MULTIPLIER.powi(attempt.saturating_sub(1) as i32);
        let capped_delay = base_delay.min(MAX_DELAY_MS as f64);

        let jitter_range = capped_delay * 0.25;
        let jitter = (rand::random::<f64>() - 0.5) * 2.0 * jitter_range;
        let final_delay = (capped_delay + jitter).max(0.0) as u64;

        Duration::from_millis(final_delay)
    }
}
