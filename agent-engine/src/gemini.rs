use chrono::{Datelike, Utc};
use common::error::AppError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";
const REQUEST_TIMEOUT_SECS: u64 = 120;

fn get_model_pricing(model: &str) -> (i64, i64, i64, i64) {
    match model {
        "gemini-2.5-flash-lite" => (10, 2, 40, 0),
        "gemini-2.5-flash" => (10, 2, 40, 0),
        "gemini-3-flash-preview" => (50, 12, 300, 14),
        "gemini-3.1-pro-preview" => (200, 50, 1200, 0),
        _ => panic!("Unknown model for pricing: {}", model),
    }
}

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
    deployment_id: Option<i64>,
    context_id: Option<i64>,
    context_group: Option<String>,
    redis_client: Option<redis::Client>,
    nats_client: Option<async_nats::Client>,
    is_byok: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiResponse {
    pub candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(rename = "modelVersion")]
    pub model_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub content: CandidateContent,
    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>,
    pub index: u32,
    #[serde(rename = "groundingMetadata", default)]
    pub grounding_metadata: Option<GroundingMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CandidateContent {
    pub parts: Vec<CandidatePart>,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CandidatePart {
    pub text: String,
    #[serde(rename = "thoughtSignature")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GroundingMetadata {
    #[serde(rename = "webSearchQueries", default)]
    pub web_search_queries: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModalityTokenCount {
    pub modality: String,
    #[serde(rename = "tokenCount")]
    pub token_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    pub prompt_token_count: u32,
    #[serde(rename = "cachedContentTokenCount", default)]
    pub cached_content_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount")]
    pub candidates_token_count: u32,
    #[serde(rename = "totalTokenCount")]
    pub total_token_count: u32,
    #[serde(rename = "thoughtsTokenCount", default)]
    pub thoughts_token_count: Option<u32>,
    #[serde(rename = "toolUsePromptTokenCount", default)]
    pub tool_use_prompt_token_count: Option<u32>,
    #[serde(rename = "promptTokensDetails", default)]
    pub prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
    #[serde(rename = "cacheTokensDetails", default)]
    pub cache_tokens_details: Option<Vec<ModalityTokenCount>>,
    #[serde(rename = "candidatesTokensDetails", default)]
    pub candidates_tokens_details: Option<Vec<ModalityTokenCount>>,
    #[serde(rename = "toolUsePromptTokensDetails", default)]
    pub tool_use_prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
}

impl GeminiClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            api_key,
            model,
            client: reqwest::Client::new(),
            deployment_id: None,
            context_id: None,
            context_group: None,
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
            context_id: None,
            context_group: None,
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

    pub fn with_context(mut self, context_id: i64, context_group: Option<String>) -> Self {
        self.context_id = Some(context_id);
        self.context_group = context_group;
        self
    }

    pub fn from_deployment(
        deployment_ai_settings: Option<&models::DeploymentAiSettings>,
        encryption_service: &common::EncryptionService,
        model: &str,
        deployment_id: i64,
        context_id: i64,
        context_group: Option<String>,
        redis_client: redis::Client,
        nats_client: async_nats::Client,
    ) -> Result<Self, AppError> {
        if let Some(encrypted_key) = deployment_ai_settings.and_then(|s| s.gemini_api_key.as_ref())
        {
            let decrypted_key = encryption_service.decrypt(encrypted_key)?;
            return Ok(Self::new_byok(decrypted_key, model.to_string())
                .with_billing(deployment_id, redis_client)
                .with_nats(nats_client)
                .with_context(context_id, context_group));
        }
        let api_key = std::env::var("GEMINI_API_KEY").unwrap();
        Ok(Self::new(api_key, model.to_string())
            .with_billing(deployment_id, redis_client)
            .with_nats(nats_client)
            .with_context(context_id, context_group))
    }

    pub async fn generate_structured_content<T>(
        &self,
        request_body: String,
    ) -> Result<(T, Option<String>), AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);

        let mut attempt = 0u32;
        const MAX_RETRIES: u32 = 3;

        let last_error = loop {
            let response = self
                .client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .header("Content-Type", "application/json")
                .body(request_body.clone())
                .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let body = resp.bytes().await;
                    match body {
                        Ok(bytes) => {
                            let raw_response = String::from_utf8_lossy(&bytes);

                            match serde_json::from_slice::<GeminiResponse>(&bytes) {
                                Ok(gemini_response) => {
                                    let mut accumulated_text = String::new();
                                    let mut thought_signature = None;

                                    for part in &gemini_response.candidates[0].content.parts {
                                        accumulated_text.push_str(&part.text);
                                        if let Some(sig) = &part.thought_signature {
                                            thought_signature = Some(sig.clone());
                                        }
                                    }

                                    if accumulated_text.is_empty() {
                                        let error =
                                            "No response content from Gemini API".to_string();

                                        attempt += 1;
                                        if attempt < MAX_RETRIES {
                                            let delay = Self::calculate_backoff_delay(attempt);
                                            tracing::warn!(
                                                attempt = attempt,
                                                delay_ms = delay.as_millis(),
                                                "Gemini API returned empty response, retrying"
                                            );
                                            tokio::time::sleep(delay).await;
                                            continue;
                                        }
                                        break error;
                                    }

                                    match serde_json::from_str::<T>(&accumulated_text) {
                                        Ok(parsed_response) => {
                                            if let Some(usage) = &gemini_response.usage_metadata {
                                                self.track_token_usage(usage, &gemini_response)
                                                    .await;
                                            }
                                            return Ok((parsed_response, thought_signature));
                                        }
                                        Err(e) => {
                                            break format!("Failed to parse response: {e}");
                                        }
                                    }
                                }
                                Err(e) => {
                                    let error = format!("Invalid API response format: {e}");
                                    tracing::error!(
                                        "Gemini API parse error: {}. Raw response (first 500 chars): {}",
                                        e,
                                        &raw_response.chars().take(500).collect::<String>()
                                    );

                                    attempt += 1;
                                    if attempt < MAX_RETRIES {
                                        let delay = Self::calculate_backoff_delay(attempt);
                                        tracing::warn!(
                                            attempt = attempt,
                                            delay_ms = delay.as_millis(),
                                            "Gemini API parse error, retrying"
                                        );
                                        tokio::time::sleep(delay).await;
                                        continue;
                                    }
                                    break error;
                                }
                            }
                        }
                        Err(e) => {
                            let error = format!("Failed to read response body: {e}");

                            attempt += 1;
                            if attempt < MAX_RETRIES {
                                let delay = Self::calculate_backoff_delay(attempt);
                                tracing::warn!(
                                    attempt = attempt,
                                    delay_ms = delay.as_millis(),
                                    "Failed to read Gemini response body, retrying"
                                );
                                tokio::time::sleep(delay).await;
                                continue;
                            }
                            break error;
                        }
                    }
                }
                Err(e) => {
                    let error = format!("Request failed: {e}");

                    attempt += 1;
                    if attempt < MAX_RETRIES {
                        let delay = Self::calculate_backoff_delay(attempt);
                        tracing::warn!(
                            attempt = attempt,
                            delay_ms = delay.as_millis(),
                            "Gemini API request failed, retrying with exponential backoff"
                        );
                        tokio::time::sleep(delay).await;
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

    async fn track_token_usage(&self, usage: &UsageMetadata, response: &GeminiResponse) {
        let Some(deployment_id) = self.deployment_id else {
            return;
        };
        let Some(redis_client) = &self.redis_client else {
            return;
        };

        if let Ok(mut conn) = redis_client.get_multiplexed_async_connection().await {
            let now = Utc::now();
            let period = format!("{}-{:02}", now.year(), now.month());
            let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

            let cached_tokens = usage.cached_content_token_count.unwrap_or(0) as i64;
            let total_prompt_tokens = usage.prompt_token_count as i64;
            let non_cached_input_tokens = total_prompt_tokens.saturating_sub(cached_tokens);
            let output_tokens = usage.candidates_token_count as i64;

            let (input_price, cached_price, output_price, search_query_price) =
                get_model_pricing(&self.model);
            let mut search_query_count = 0i64;
            let mut unique_search_queries = std::collections::HashSet::new();
            for candidate in &response.candidates {
                if let Some(grounding) = &candidate.grounding_metadata {
                    if let Some(queries) = &grounding.web_search_queries {
                        for query in queries {
                            let trimmed = query.trim();
                            if !trimmed.is_empty() {
                                search_query_count += 1;
                                unique_search_queries.insert(trimmed.to_lowercase());
                            }
                        }
                    }
                }
            }
            let search_query_unique_count = unique_search_queries.len() as i64;

            let non_cached_cost = (non_cached_input_tokens * input_price) / 1_000_000;
            let cached_cost = (cached_tokens * cached_price) / 1_000_000;
            let input_cost_cents = non_cached_cost + cached_cost;

            let output_cost_cents = (output_tokens * output_price) / 1_000_000;
            let search_query_cost_cents = (search_query_count * search_query_price) / 1_000;

            let webhook_payload = serde_json::json!({
                "model": self.model,
                "is_byok": self.is_byok,
                "context_id": self.context_id,
                "context_group": self.context_group,
                "input_tokens": total_prompt_tokens,
                "cached_tokens": cached_tokens,
                "output_tokens": output_tokens,
                "input_cost_cents": input_cost_cents,
                "output_cost_cents": output_cost_cents,
                "search_query_count": search_query_count,
                "search_query_unique_count": search_query_unique_count,
                "search_query_cost_cents": search_query_cost_cents,
                "total_cost_cents": input_cost_cents + output_cost_cents + search_query_cost_cents,
                "timestamp": now.to_rfc3339(),
                "prompt_token_count": usage.prompt_token_count,
                "candidates_token_count": usage.candidates_token_count,
                "total_token_count": usage.total_token_count,
                "cached_content_token_count": usage.cached_content_token_count,
                "thoughts_token_count": usage.thoughts_token_count,
            });

            if let Some(nats_client) = &self.nats_client {
                let task_message = dto::json::NatsTaskMessage {
                    task_type: "webhook.event".to_string(),
                    task_id: format!(
                        "model-usage-{}-{}",
                        deployment_id,
                        Utc::now().timestamp_micros()
                    ),
                    payload: serde_json::json!({
                        "deployment_id": deployment_id,
                        "event_type": "agent.model.usage",
                        "event_payload": webhook_payload,
                        "triggered_at": now.to_rfc3339(),
                    }),
                };

                let message_bytes = serde_json::to_vec(&task_message).unwrap_or_default();
                if !message_bytes.is_empty() {
                    let _ = nats_client
                        .publish("worker.tasks.webhook.event", message_bytes.into())
                        .await;
                }
            }

            if !self.is_byok {
                let mut pipe = redis::pipe();
                pipe.atomic()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_token_input_cost_cents",
                        input_cost_cents,
                    )
                    .ignore()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_token_output_cost_cents",
                        output_cost_cents,
                    )
                    .ignore()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_search_queries",
                        search_query_count,
                    )
                    .ignore()
                    .zincr(
                        &format!("{}:metrics", prefix),
                        "ai_search_query_cost_cents",
                        search_query_cost_cents,
                    )
                    .ignore()
                    .expire(&format!("{}:metrics", prefix), 5184000)
                    .ignore()
                    .zincr(
                        &format!("billing:{}:dirty_deployments", period),
                        deployment_id,
                        input_cost_cents + output_cost_cents + search_query_cost_cents,
                    )
                    .ignore()
                    .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
                    .ignore();

                let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
            }
        }
    }
}
