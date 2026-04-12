use chrono::{DateTime, Utc};
use common::error::AppError;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tracing::info;

use super::{ExplicitCacheRequest, GeminiClient};
use super::types::{CachedContentResponse, ExplicitCachePlan, PreparedGenerateRequest};

const GEMINI_API_ROOT_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const REQUEST_TIMEOUT_SECS: u64 = 240;
const EXPLICIT_CACHE_MIN_TOKENS_FLASH: usize = 1_024;
const EXPLICIT_CACHE_MIN_TOKENS_PRO: usize = 4_096;
const EXPLICIT_CACHE_ESTIMATED_CHARS_PER_TOKEN: usize = 4;

impl GeminiClient {
    pub(crate) async fn prepare_generate_request_body(
        &self,
        request_body: String,
        cache_request: Option<&ExplicitCacheRequest>,
    ) -> PreparedGenerateRequest {
        let Some(cache_request) = cache_request else {
            return PreparedGenerateRequest {
                request_body,
                cache_plan: None,
            };
        };

        let Some(plan) = self.build_explicit_cache_plan(&request_body, cache_request) else {
            return PreparedGenerateRequest {
                request_body,
                cache_plan: None,
            };
        };

        let request_body =
            serde_json::to_string(&plan.send_request_payload).unwrap_or(request_body);
        if let Some(cache_state) = cache_request.prior_state.as_ref() {
            if self.can_use_cached_prefix(cache_request, cache_state, &plan) {
            }
        }

        PreparedGenerateRequest {
            request_body,
            cache_plan: Some(plan),
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
        let tool_config = send_request_object
            .get("tool_config")
            .cloned()
            .or_else(|| send_request_object.get("toolConfig").cloned());

        let contents = send_request_object
            .get("contents")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();

        if contents.is_empty() {
            return None;
        }

        if system_instruction.is_none()
            && contents.is_empty()
            && tools.is_none()
            && tool_config.is_none()
        {
            return None;
        }

        let cacheable_content_count = contents
            .len()
            .saturating_sub(cache_request.live_tail_count.min(contents.len()));
        let cacheable_contents = contents[..cacheable_content_count].to_vec();
        let live_tail_contents = contents[cacheable_content_count..].to_vec();

        let stable_payload = json!({
            "systemInstruction": system_instruction.clone(),
            "tools": tools.clone(),
            "toolConfig": tool_config.clone(),
        });
        let prefix_signature =
            self.short_hash(serde_json::to_string(&stable_payload).ok()?.as_bytes(), 32);
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
                self.short_hash(request_body.as_bytes(), 12)
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

        if let Some(tool_config) = tool_config {
            full_cache_payload.insert("toolConfig".to_string(), tool_config);
        }

        let full_cache_payload = Value::Object(full_cache_payload);
        let estimated_prefix_tokens = Self::estimate_tokens(&full_cache_payload);
        if estimated_prefix_tokens < Self::explicit_cache_min_tokens_for_model(&self.model) {
            return None;
        }

        if let Some(prior_state) = cache_request.prior_state.as_ref() {
            if self.can_use_cached_prefix(
                cache_request,
                prior_state,
                &ExplicitCachePlan {
                    full_cache_payload: full_cache_payload.clone(),
                    send_request_payload: Value::Null,
                    prefix_signature: prefix_signature.clone(),
                    cached_contents_signature: cached_contents_signature.clone(),
                    cached_content_count: cacheable_contents.len(),
                },
            ) {
                let mut delta_contents =
                    cacheable_contents[prior_state.cached_content_count..].to_vec();
                delta_contents.extend(live_tail_contents.clone());
                send_request_object.remove("system_instruction");
                send_request_object.remove("systemInstruction");
                send_request_object.remove("tools");
                send_request_object.remove("tool_config");
                send_request_object.remove("toolConfig");
                send_request_object
                    .insert("cachedContent".to_string(), json!(prior_state.cache_name));
                send_request_object.insert("contents".to_string(), Value::Array(delta_contents));
            }
        }

        Some(ExplicitCachePlan {
            full_cache_payload,
            send_request_payload,
            prefix_signature,
            cached_contents_signature,
            cached_content_count: cacheable_contents.len(),
        })
    }

    fn can_use_cached_prefix(
        &self,
        cache_request: &ExplicitCacheRequest,
        prior_state: &models::PromptCacheState,
        plan: &ExplicitCachePlan,
    ) -> bool {
        if prior_state.cache_key != cache_request.cache_key
            || prior_state.model_name != self.model
            || prior_state.expire_at <= Utc::now() + chrono::Duration::seconds(5)
            || prior_state.prefix_signature != plan.prefix_signature
            || plan.cached_content_count < prior_state.cached_content_count
        {
            return false;
        }

        if plan.cached_content_count == prior_state.cached_content_count
            && cache_request.live_tail_count == 0
        {
            return false;
        }

        let current_request = match plan
            .full_cache_payload
            .get("contents")
            .and_then(|v| v.as_array())
        {
            Some(contents) => contents,
            None => return false,
        };

        let cached_prefix = current_request[..prior_state.cached_content_count].to_vec();
        let prefix_signature = match serde_json::to_string(&cached_prefix)
            .ok()
            .map(|s| self.short_hash(s.as_bytes(), 32))
        {
            Some(signature) => signature,
            None => return false,
        };

        prefix_signature == prior_state.cached_contents_signature
    }

    pub(crate) async fn refresh_explicit_cache(
        &self,
        cache_request: &ExplicitCacheRequest,
        plan: &ExplicitCachePlan,
    ) -> Result<Option<models::PromptCacheState>, AppError> {
        if let Some(prior_state) = cache_request.prior_state.as_ref() {
            let same_snapshot = prior_state.cache_key == cache_request.cache_key
                && prior_state.model_name == self.model
                && prior_state.prefix_signature == plan.prefix_signature
                && prior_state.cached_contents_signature == plan.cached_contents_signature
                && prior_state.cached_content_count == plan.cached_content_count
                && prior_state.expire_at > Utc::now() + chrono::Duration::seconds(5);
            if same_snapshot {
                return Ok(Some(prior_state.clone()));
            }
        }

        let cache_url = format!("{}/cachedContents", GEMINI_API_ROOT_URL);
        let serialized_payload = serde_json::to_string(&plan.full_cache_payload).map_err(|e| {
            AppError::Internal(format!("Failed to serialize Gemini cache payload: {e}"))
        })?;
        info!(
            "{}",
            json!({
                "event": "gemini_cache_create_request",
                "model": self.model,
                "url": cache_url,
                "cache_key": cache_request.cache_key,
                "ttl_secs": cache_request.ttl_secs,
                "live_tail_count": cache_request.live_tail_count,
                "reuse_only": cache_request.reuse_only,
                "request": plan.full_cache_payload,
            })
            .to_string()
        );
        let cache_response = self
            .client
            .post(&cache_url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .body(serialized_payload)
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Gemini cache create request failed: {e}")))?;

        if !cache_response.status().is_success() {
            let status = cache_response.status();
            let body = cache_response.text().await.unwrap_or_default();
            info!(
                "{}",
                json!({
                    "event": "gemini_cache_create_response",
                    "model": self.model,
                    "url": cache_url,
                    "cache_key": cache_request.cache_key,
                    "status": status.as_u16(),
                    "ok": false,
                    "response": body,
                })
                .to_string()
            );
            return Err(AppError::Internal(format!(
                "Gemini cache create request failed with status {}: {}",
                status,
                body.chars().take(2000).collect::<String>()
            )));
        }

        let response_body = cache_response.text().await.map_err(|e| {
            AppError::Internal(format!("Failed to read Gemini cache create response: {e}"))
        })?;
        info!(
            "{}",
            json!({
                "event": "gemini_cache_create_response",
                "model": self.model,
                "url": cache_url,
                "cache_key": cache_request.cache_key,
                "status": 200,
                "ok": true,
                "response": response_body,
            })
            .to_string()
        );
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

        let handle = models::PromptCacheState {
            cache_key: cache_request.cache_key.clone(),
            model_name: self.model.clone(),
            cache_name: parsed.name.clone(),
            prefix_signature: plan.prefix_signature.clone(),
            cached_contents_signature: plan.cached_contents_signature.clone(),
            cached_content_count: plan.cached_content_count,
            expire_at,
        };

        Ok(Some(handle))
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
