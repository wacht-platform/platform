use chrono::Utc;
use common::error::AppError;
use models::PromptCacheState;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

use super::{gemini::ExplicitCacheRequest, provider::LlmProvider, UsageMetadata};

pub type LlmClient = Arc<dyn LlmProvider>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmRole {
    Strong,
    Weak,
}

#[derive(Debug, Clone)]
pub struct PromptCacheLayer {
    pub cache_key: String,
    pub ttl_secs: i64,
    pub live_tail_count: usize,
    pub prior_state: Option<PromptCacheState>,
    pub incremental: bool,
}

#[derive(Debug, Clone)]
pub struct PromptCacheRequest {
    pub shared: PromptCacheLayer,
    pub incremental: PromptCacheLayer,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedToolCall {
    pub tool_name: String,
    pub arguments: Value,
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallGenerationOutput {
    pub calls: Vec<GeneratedToolCall>,
    /// Model's free-form text output. Present when the model wrote a text response
    /// alongside (or instead of) tool calls. In the unified ReAct loop, text with no
    /// tool calls signals the terminal user-facing response.
    #[serde(default)]
    pub content_text: Option<String>,
    pub usage_metadata: Option<UsageMetadata>,
    /// Shared + incremental handles to persist. Empty when caching is off.
    #[serde(default)]
    pub cache_states: Vec<PromptCacheState>,
    /// Provider finish reason for the turn (e.g. "STOP"/"length"/"MAX_TOKENS").
    /// Used by the loop to detect truncated turns. None when the provider omits it.
    #[serde(default)]
    pub finish_reason: Option<String>,
}

fn provider_cached_tokens(usage: Option<&UsageMetadata>) -> Option<u32> {
    usage.and_then(|meta| meta.cached_content_token_count.or(meta.cache_write_token_count))
}

/// Stamp generate-time cached tokens only on the attached Gemini object.
/// The other layer keeps create/prior counts so shared ≠ incremental size.
pub fn stamp_attached_cache_tokens(
    mut states: Vec<PromptCacheState>,
    attached_cache_name: Option<&str>,
    usage: Option<&UsageMetadata>,
) -> Vec<PromptCacheState> {
    let Some(attached) = attached_cache_name.filter(|name| !name.is_empty()) else {
        return states;
    };
    let tokens = provider_cached_tokens(usage);
    for state in &mut states {
        if state.cache_name == attached {
            state.record_provider_cached_tokens(tokens);
        }
    }
    states
}

fn layer_state_from_request(
    layer: &PromptCacheLayer,
    model_name: &str,
    usage: Option<&UsageMetadata>,
) -> Option<PromptCacheState> {
    let expire_at = layer
        .prior_state
        .as_ref()
        .map(|state| state.expire_at)
        .unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(layer.ttl_secs.max(1)));
    let mut state = layer.prior_state.clone().unwrap_or_else(|| PromptCacheState {
        cache_key: layer.cache_key.clone(),
        model_name: model_name.to_string(),
        cache_name: String::new(),
        prefix_signature: String::new(),
        cached_contents_signature: String::new(),
        cached_content_count: 0,
        expire_at,
        reuse_turns: 0,
        cached_token_count: None,
    });
    state.record_provider_cached_tokens(provider_cached_tokens(usage));
    // Skip empty handles so a cache miss does not overwrite a prior count.
    state.cached_token_count.is_some().then_some(state)
}

/// OpenAI / OpenRouter have no Gemini `cachedContents` resource. Persist the
/// thread-level handle (the key we send) with the provider-reported count.
pub fn cache_states_from_request(
    cache: Option<PromptCacheRequest>,
    model_name: &str,
    usage: Option<&UsageMetadata>,
) -> Vec<PromptCacheState> {
    let Some(cache) = cache else {
        return Vec::new();
    };
    layer_state_from_request(&cache.incremental, model_name, usage)
        .into_iter()
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticLlmContentBlock {
    Text {
        text: String,
    },
    InlineData {
        mime_type: String,
        data: String,
    },
    ToolCall {
        id: String,
        name: String,
        args: Value,
        #[serde(default)]
        signature: Option<String>,
        #[serde(default)]
        origin_provider: Option<String>,
        #[serde(default)]
        origin_model: Option<String>,
    },
    ToolResult {
        call_id: String,
        name: String,
        output: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticLlmMessage {
    pub role: String,
    #[serde(default)]
    pub content_blocks: Vec<SemanticLlmContentBlock>,
}

impl SemanticLlmMessage {
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content_blocks: vec![SemanticLlmContentBlock::Text {
                text: content.into(),
            }],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticLlmPromptConfig {
    pub response_json_schema: Value,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticLlmRequest {
    pub system_prompt: String,
    #[serde(default)]
    pub messages: Vec<SemanticLlmMessage>,
    pub response_json_schema: Value,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub forced_tool_names: Option<Vec<String>>,
}

impl SemanticLlmRequest {
    pub fn from_config(
        system_prompt: String,
        messages: Vec<SemanticLlmMessage>,
        config: SemanticLlmPromptConfig,
    ) -> Self {
        Self {
            system_prompt,
            messages,
            response_json_schema: config.response_json_schema,
            temperature: config.temperature,
            max_output_tokens: config.max_output_tokens,
            reasoning_effort: config.reasoning_effort,
            forced_tool_names: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredGenerationOutput<T> {
    pub value: T,
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(default)]
    pub cache_states: Vec<PromptCacheState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextGenerationOutput {
    pub text: String,
    pub usage_metadata: Option<UsageMetadata>,
}

#[derive(Clone)]
pub struct ResolvedLlm {
    client: LlmClient,
    model_name: String,
}

impl std::fmt::Debug for ResolvedLlm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedLlm")
            .field("provider", &self.client.provider_label())
            .field("model_name", &self.model_name)
            .finish()
    }
}

impl ResolvedLlm {
    pub fn new(client: LlmClient, model_name: impl Into<String>) -> Self {
        Self {
            client,
            model_name: model_name.into(),
        }
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub fn provider_label(&self) -> &'static str {
        self.client.provider_label()
    }

    pub async fn generate_structured_from_prompt<T>(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<StructuredGenerationOutput<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        const STRUCTURED_TOOL: &str = "submit_structured_output";
        let tool = NativeToolDefinition {
            name: STRUCTURED_TOOL.to_string(),
            description: "Return the result by calling this tool with arguments that match the \
                          schema. Do not reply with free-form text."
                .to_string(),
            input_schema: prompt.response_json_schema.clone(),
        };
        let mut prompt = prompt;
        prompt.forced_tool_names = Some(vec![STRUCTURED_TOOL.to_string()]);

        let output = self
            .client
            .generate_tool_calls(prompt, vec![tool], cache)
            .await?;
        let usage_metadata = output.usage_metadata;
        let cache_states = output.cache_states;
        let mut calls = output.calls;

        let idx = calls
            .iter()
            .position(|c| c.tool_name == STRUCTURED_TOOL || c.tool_name.ends_with(STRUCTURED_TOOL))
            .or_else(|| (calls.len() == 1).then_some(0));
        let Some(idx) = idx else {
            return Err(AppError::Internal(
                "structured generation: model returned no usable tool call".to_string(),
            ));
        };
        let arguments = calls.swap_remove(idx).arguments;

        let value: T = serde_json::from_value(arguments).map_err(|e| {
            AppError::Internal(format!("Failed to deserialize structured output: {e}"))
        })?;
        Ok(StructuredGenerationOutput {
            value,
            usage_metadata,
            cache_states,
        })
    }

    #[tracing::instrument(
        name = "llm.generate_tool_calls",
        skip(self, prompt, tools, cache),
        fields(
            provider = self.client.provider_label(),
            tool_count = tools.len(),
            empty_response = tracing::field::Empty,
            tool_call_count = tracing::field::Empty,
        )
    )]
    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
        cache: Option<PromptCacheRequest>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        let result = self.client.generate_tool_calls(prompt, tools, cache).await;
        if let Ok(output) = &result {
            let span = tracing::Span::current();
            span.record("tool_call_count", output.calls.len());
            span.record(
                "empty_response",
                output.calls.is_empty()
                    && output
                        .content_text
                        .as_deref()
                        .map(|t| t.trim().is_empty())
                        .unwrap_or(true),
            );
        }
        result
    }

    pub async fn generate_text_from_prompt(
        &self,
        prompt: SemanticLlmRequest,
    ) -> Result<TextGenerationOutput, AppError> {
        self.client.generate_text(prompt).await
    }

    pub async fn delete_prompt_cache(&self, cache_name: &str) {
        self.client.delete_prompt_cache(cache_name).await;
    }
}

impl From<&PromptCacheLayer> for ExplicitCacheRequest {
    fn from(layer: &PromptCacheLayer) -> Self {
        Self {
            cache_key: layer.cache_key.clone(),
            ttl_secs: layer.ttl_secs,
            live_tail_count: layer.live_tail_count,
            prior_state: layer.prior_state.clone(),
            incremental: layer.incremental,
        }
    }
}
