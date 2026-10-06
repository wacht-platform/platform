use chrono::{DateTime, Utc};
use common::error::AppError;
use common::ResultExt;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::types::{CachedContentResponse, ExplicitCachePlan, PreparedGenerateRequest};
use super::{ExplicitCacheRequest, GeminiClient};

struct PreparedLayer {
    request_body: String,
    state: Option<models::PromptCacheState>,
    attachable: bool,
}

const GEMINI_API_ROOT_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const REQUEST_TIMEOUT_SECS: u64 = 240;
const EXPLICIT_CACHE_MIN_TOKENS_FLASH: usize = 4_096;
const EXPLICIT_CACHE_MIN_TOKENS_PRO: usize = 4_096;
const EXPLICIT_CACHE_ESTIMATED_CHARS_PER_TOKEN: usize = 4;
/// PATCH TTL when less than this remains, so a 1-day cache never dies mid-run.
const EXPLICIT_CACHE_RENEW_LEAD_SECS: i64 = 2 * 3_600;

impl GeminiClient {
    pub(crate) async fn prepare_generate_request_body(
        &self,
        request_body: String,
        cache_request: Option<&crate::llm::PromptCacheRequest>,
    ) -> PreparedGenerateRequest {
        let Some(cache_request) = cache_request else {
            return Self::uncached_prepare(request_body);
        };

        let incremental = ExplicitCacheRequest::from(&cache_request.incremental);
        if let Some(prepared) = self.prepare_single_layer(&request_body, &incremental).await {
            if prepared.attachable {
                return Self::attached_prepare(prepared);
            }
        }

        let shared = ExplicitCacheRequest::from(&cache_request.shared);
        if let Some(prepared) = self.prepare_single_layer(&request_body, &shared).await {
            if prepared.attachable {
                return Self::attached_prepare(prepared);
            }
        }

        Self::uncached_prepare(request_body)
    }

    fn attached_prepare(prepared: PreparedLayer) -> PreparedGenerateRequest {
        PreparedGenerateRequest {
            request_body: prepared.request_body,
            attached_cache_name: prepared
                .state
                .as_ref()
                .map(|state| state.cache_name.clone()),
            cache_states: prepared.state.into_iter().collect(),
        }
    }

    async fn prepare_single_layer(
        &self,
        request_body: &str,
        cache_request: &ExplicitCacheRequest,
    ) -> Option<PreparedLayer> {
        let mut plan = self.build_explicit_cache_plan(request_body, cache_request)?;

        let cache_state = match self.ensure_explicit_cache(cache_request, &plan).await {
            Ok(state) => state,
            Err(error) => {
                tracing::warn!(
                    model = %self.model,
                    cache_key = %cache_request.cache_key,
                    error = %error,
                    "Gemini explicit cache create/renew failed; leaving this layer unused"
                );
                return None;
            }
        };

        if let Some(state) = cache_state.as_ref() {
            if let Some(object) = plan.send_request_payload.as_object_mut() {
                object.insert("cachedContent".to_string(), json!(state.cache_name));
            }
        }

        let attachable = cache_state
            .as_ref()
            .is_some_and(|state| !state.cache_name.is_empty());
        let request_body =
            serde_json::to_string(&plan.send_request_payload).unwrap_or_else(|_| request_body.to_string());
        Some(PreparedLayer {
            request_body,
            state: cache_state,
            attachable,
        })
    }

    fn uncached_prepare(request_body: String) -> PreparedGenerateRequest {
        PreparedGenerateRequest {
            request_body,
            cache_states: Vec::new(),
            attached_cache_name: None,
        }
    }

    fn build_explicit_cache_plan(
        &self,
        request_body: &str,
        cache_request: &ExplicitCacheRequest,
    ) -> Option<ExplicitCachePlan> {
        let mut send_request_payload = serde_json::from_str::<Value>(request_body).ok()?;
        let send_request_object = send_request_payload.as_object_mut()?;

        if send_request_object.get("cachedContent").is_some()
            || send_request_object.get("cached_content").is_some()
        {
            return None;
        }

        let system_instruction = send_request_object
            .get("system_instruction")
            .cloned()
            .or_else(|| send_request_object.get("systemInstruction").cloned());

        let tools = send_request_object.get("tools").cloned();
        // toolConfig stays on the generate request: forced-tool nudges must not
        // bust the shared agent cache.

        let contents = send_request_object
            .get("contents")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();

        if system_instruction.is_none() && tools.is_none() && contents.is_empty() {
            return None;
        }

        let cacheable_content_count = contents
            .len()
            .saturating_sub(cache_request.live_tail_count.min(contents.len()));
        let cacheable_contents = contents[..cacheable_content_count].to_vec();
        let live_tail_contents = contents[cacheable_content_count..].to_vec();

        let prefix_signature = if cache_request.incremental {
            self.short_hash(
                serde_json::to_string(&json!({
                    "systemInstruction": system_instruction.clone(),
                    "tools": tools.clone(),
                }))
                .ok()?
                .as_bytes(),
                32,
            )
        } else {
            self.short_hash(
                serde_json::to_string(&json!({
                    "systemInstruction": system_instruction.clone(),
                    "tools": tools.clone(),
                    "contents": cacheable_contents.clone(),
                }))
                .ok()?
                .as_bytes(),
                32,
            )
        };
        let cached_contents_signature = self.short_hash(
            serde_json::to_string(&cacheable_contents).ok()?.as_bytes(),
            32,
        );

        let mut full_cache_payload = serde_json::Map::new();
        full_cache_payload.insert("model".to_string(), json!(format!("models/{}", self.model)));
        full_cache_payload.insert(
            "displayName".to_string(),
            json!(format!(
                "agent-engine-{}",
                self.short_hash(cache_request.cache_key.as_bytes(), 12)
            )),
        );
        full_cache_payload.insert(
            "ttl".to_string(),
            json!(format!("{}s", cache_request.ttl_secs.max(1))),
        );

        if let Some(system_instruction) = system_instruction {
            full_cache_payload.insert("systemInstruction".to_string(), system_instruction);
        }

        if !cacheable_contents.is_empty() {
            full_cache_payload.insert(
                "contents".to_string(),
                Value::Array(cacheable_contents.clone()),
            );
        }

        if let Some(tools) = tools {
            full_cache_payload.insert("tools".to_string(), tools);
        }

        let full_cache_payload = Value::Object(full_cache_payload);
        let estimated_prefix_tokens = cache_request
            .prior_state
            .as_ref()
            .and_then(|state| state.cached_token_count)
            .filter(|tokens| *tokens > 0)
            .map(|tokens| tokens as usize)
            .unwrap_or_else(|| {
                Self::estimate_tokens(&json!({
                    "systemInstruction": full_cache_payload.get("systemInstruction"),
                    "tools": full_cache_payload.get("tools"),
                    "contents": full_cache_payload.get("contents"),
                }))
            });
        if estimated_prefix_tokens < Self::explicit_cache_min_tokens_for_model(&self.model) {
            return None;
        }

        let mut should_refresh = true;
        let mut should_renew_ttl = false;
        if let Some(prior_state) = cache_request.prior_state.as_ref() {
            if self.can_use_cached_prefix(
                cache_request,
                prior_state,
                &prefix_signature,
                &cached_contents_signature,
                cacheable_contents.len(),
                &full_cache_payload,
            ) {
                let near_expiry = prior_state.expire_at
                    <= Utc::now() + chrono::Duration::seconds(EXPLICIT_CACHE_RENEW_LEAD_SECS);
                let generate_contents = if cache_request.incremental {
                    let delta_slice =
                        cacheable_contents[prior_state.cached_content_count..].to_vec();
                    let delta_tokens = Self::estimate_tokens(&Value::Array(delta_slice.clone()));
                    let cached_prefix_tokens = prior_state
                        .cached_token_count
                        .filter(|tokens| *tokens > 0)
                        .map(|tokens| tokens as usize)
                        .unwrap_or(estimated_prefix_tokens);
                    let reuse_turns = (prior_state.reuse_turns as usize).saturating_add(1);
                    should_refresh = Self::incremental_should_recache(
                        delta_tokens,
                        reuse_turns,
                        cached_prefix_tokens,
                    );
                    if should_refresh {
                        live_tail_contents.clone()
                    } else {
                        should_renew_ttl = near_expiry;
                        let mut reuse_contents = delta_slice;
                        reuse_contents.extend(live_tail_contents.iter().cloned());
                        reuse_contents
                    }
                } else {
                    should_refresh = false;
                    should_renew_ttl = near_expiry;
                    live_tail_contents.clone()
                };

                send_request_object.remove("system_instruction");
                send_request_object.remove("systemInstruction");
                send_request_object.remove("tools");
                send_request_object.insert(
                    "cachedContent".to_string(),
                    json!(prior_state.cache_name),
                );
                send_request_object
                    .insert("contents".to_string(), Value::Array(generate_contents));
            }
        }

        if should_refresh {
            send_request_object.remove("system_instruction");
            send_request_object.remove("systemInstruction");
            send_request_object.remove("tools");
            send_request_object.remove("cachedContent");
            send_request_object.insert(
                "contents".to_string(),
                Value::Array(live_tail_contents),
            );
        }

        Some(ExplicitCachePlan {
            full_cache_payload,
            send_request_payload,
            prefix_signature,
            cached_contents_signature,
            cached_content_count: cacheable_contents.len(),
            should_refresh,
            should_renew_ttl,
        })
    }

    fn can_use_cached_prefix(
        &self,
        cache_request: &ExplicitCacheRequest,
        prior_state: &models::PromptCacheState,
        prefix_signature: &str,
        cached_contents_signature: &str,
        cached_content_count: usize,
        full_cache_payload: &Value,
    ) -> bool {
        if prior_state.cache_key != cache_request.cache_key
            || prior_state.model_name != self.model
            || prior_state.expire_at <= Utc::now() + chrono::Duration::seconds(5)
            || prior_state.prefix_signature != prefix_signature
        {
            return false;
        }

        if !cache_request.incremental {
            return prior_state.cached_contents_signature == cached_contents_signature
                && prior_state.cached_content_count == cached_content_count;
        }

        if cached_content_count < prior_state.cached_content_count {
            return false;
        }
        if cached_content_count == prior_state.cached_content_count
            && cache_request.live_tail_count == 0
        {
            return false;
        }

        let Some(contents) = full_cache_payload
            .get("contents")
            .and_then(|value| value.as_array())
        else {
            return prior_state.cached_content_count == 0;
        };
        if contents.len() < prior_state.cached_content_count {
            return false;
        }
        let cached_prefix = contents[..prior_state.cached_content_count].to_vec();
        let Some(prefix_hash) = serde_json::to_string(&cached_prefix)
            .ok()
            .map(|serialized| self.short_hash(serialized.as_bytes(), 32))
        else {
            return false;
        };
        prefix_hash == prior_state.cached_contents_signature
    }

    async fn ensure_explicit_cache(
        &self,
        cache_request: &ExplicitCacheRequest,
        plan: &ExplicitCachePlan,
    ) -> Result<Option<models::PromptCacheState>, AppError> {
        if plan.should_refresh {
            return self.create_explicit_cache(cache_request, plan).await;
        }

        let Some(prior_state) = cache_request.prior_state.as_ref() else {
            return Ok(None);
        };

        if plan.should_renew_ttl {
            match self
                .renew_explicit_cache_ttl(&prior_state.cache_name, cache_request.ttl_secs)
                .await
            {
                Ok(expire_at) => {
                    let mut renewed = prior_state.clone();
                    renewed.expire_at = expire_at;
                    renewed.reuse_turns = renewed.reuse_turns.saturating_add(1);
                    return Ok(Some(renewed));
                }
                Err(error) => {
                    tracing::warn!(
                        cache_name = %prior_state.cache_name,
                        error = %error,
                        "Gemini cache TTL renew failed; reusing remaining TTL"
                    );
                }
            }
        }

        let mut aged = prior_state.clone();
        aged.reuse_turns = aged.reuse_turns.saturating_add(1);
        Ok(Some(aged))
    }

    async fn create_explicit_cache(
        &self,
        cache_request: &ExplicitCacheRequest,
        plan: &ExplicitCachePlan,
    ) -> Result<Option<models::PromptCacheState>, AppError> {
        let cache_url = format!("{}/cachedContents", GEMINI_API_ROOT_URL);
        let serialized_payload = serde_json::to_string(&plan.full_cache_payload).map_err(|e| {
            AppError::Internal(format!("Failed to serialize Gemini cache payload: {e}"))
        })?;
        let cache_response = self
            .client
            .post(&cache_url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .body(serialized_payload)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .map_err_internal("Gemini cache create request failed")?;

        if !cache_response.status().is_success() {
            let status = cache_response.status();
            let body = cache_response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Gemini cache create request failed with status {}: {}",
                status,
                body.chars().take(2000).collect::<String>()
            )));
        }

        let response_body = cache_response.text().await.map_err(|e| {
            AppError::Internal(format!("Failed to read Gemini cache create response: {e}"))
        })?;
        let parsed: CachedContentResponse = serde_json::from_str(&response_body).map_err(|e| {
            AppError::Internal(format!(
                "Failed to parse Gemini cache create response JSON: {}. Raw body (first 2000 chars): {}",
                e,
                response_body.chars().take(2000).collect::<String>()
            ))
        })?;

        let expire_at = parsed
            .expire_time
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value: DateTime<chrono::FixedOffset>| value.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(cache_request.ttl_secs));

        let mut handle = models::PromptCacheState {
            cache_key: cache_request.cache_key.clone(),
            model_name: self.model.clone(),
            cache_name: parsed.name.clone(),
            prefix_signature: plan.prefix_signature.clone(),
            cached_contents_signature: plan.cached_contents_signature.clone(),
            cached_content_count: plan.cached_content_count,
            expire_at,
            reuse_turns: 0,
            cached_token_count: None,
        };
        handle.record_provider_cached_tokens(
            parsed
                .usage_metadata
                .as_ref()
                .and_then(|usage| usage.total_token_count.or(usage.cached_content_token_count)),
        );

        if let Some(prior) = cache_request.prior_state.as_ref() {
            if prior.cache_name != handle.cache_name {
                self.delete_explicit_cache(&prior.cache_name).await;
            }
        }

        Ok(Some(handle))
    }

    async fn renew_explicit_cache_ttl(
        &self,
        cache_name: &str,
        ttl_secs: i64,
    ) -> Result<DateTime<Utc>, AppError> {
        let url = format!("{GEMINI_API_ROOT_URL}/{cache_name}");
        let body = json!({ "ttl": format!("{}s", ttl_secs.max(1)) });
        let response = self
            .client
            .patch(&url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .map_err_internal("Gemini cache TTL renew request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Gemini cache TTL renew failed with status {}: {}",
                status,
                body.chars().take(500).collect::<String>()
            )));
        }

        let response_body = response.text().await.unwrap_or_default();
        let parsed: CachedContentResponse = serde_json::from_str(&response_body).unwrap_or(
            CachedContentResponse {
                name: cache_name.to_string(),
                expire_time: None,
                usage_metadata: None,
            },
        );
        let expire_at = parsed
            .expire_time
            .as_deref()
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
            .map(|value: DateTime<chrono::FixedOffset>| value.with_timezone(&Utc))
            .unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(ttl_secs.max(1)));
        Ok(expire_at)
    }

    pub(crate) async fn delete_explicit_cache(&self, cache_name: &str) {
        if cache_name.is_empty() {
            return;
        }
        let url = format!("{GEMINI_API_ROOT_URL}/{cache_name}");
        match self
            .client
            .delete(&url)
            .header("x-goog-api-key", &self.api_key)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() || resp.status().as_u16() == 404 => {}
            Ok(resp) => {
                tracing::warn!(cache_name, status = %resp.status(), "Gemini cache delete failed")
            }
            Err(error) => {
                tracing::warn!(cache_name, %error, "Gemini cache delete request errored")
            }
        }
    }

    fn explicit_cache_min_tokens_for_model(model: &str) -> usize {
        let model = model.to_ascii_lowercase();
        if model.contains("flash") {
            EXPLICIT_CACHE_MIN_TOKENS_FLASH
        } else if model.contains("pro") {
            EXPLICIT_CACHE_MIN_TOKENS_PRO
        } else {
            EXPLICIT_CACHE_MIN_TOKENS_PRO
        }
    }

    fn incremental_should_recache(
        delta_tokens: usize,
        reuse_turns: usize,
        cached_prefix_tokens: usize,
    ) -> bool {
        delta_tokens.saturating_mul(reuse_turns) >= cached_prefix_tokens
    }

    fn estimate_tokens(value: &Value) -> usize {
        let approximate_chars = serde_json::to_string(value)
            .map(|serialized| serialized.chars().count())
            .unwrap_or_default();
        approximate_chars.div_ceil(EXPLICIT_CACHE_ESTIMATED_CHARS_PER_TOKEN)
    }

    fn short_hash(&self, bytes: &[u8], length: usize) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let digest = hasher.finalize();
        let mut encoded = String::with_capacity(digest.len() * 2);
        for byte in digest {
            encoded.push_str(&format!("{byte:02x}"));
        }
        encoded.chars().take(length).collect()
    }
}
