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
pub struct PromptCacheRequest {
    pub cache_key: String,
    pub ttl_secs: i64,
    pub live_tail_count: usize,
    pub prior_state: Option<PromptCacheState>,
    pub reuse_only: bool,
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
    #[serde(default)]
    pub cache_state: Option<PromptCacheState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticLlmContentBlock {
    Text { text: String },
    InlineData { mime_type: String, data: String },
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
    /// When set, forces Gemini's `toolConfig.functionCallingConfig.mode` to `ANY`
    /// and constrains `allowed_function_names` to this list. Use for narrow
    /// dispatch calls where text fallback is not acceptable.
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
    pub cache_state: Option<PromptCacheState>,
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
        let raw = self.client.generate_structured(prompt, cache).await?;
        let value: T = serde_json::from_value(raw.value).map_err(|e| {
            AppError::Internal(format!("Failed to deserialize structured output: {e}"))
        })?;
        Ok(StructuredGenerationOutput {
            value,
            usage_metadata: raw.usage_metadata,
            cache_state: raw.cache_state,
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
}

impl From<PromptCacheRequest> for ExplicitCacheRequest {
    fn from(value: PromptCacheRequest) -> Self {
        Self {
            cache_key: value.cache_key,
            ttl_secs: value.ttl_secs,
            live_tail_count: value.live_tail_count,
            prior_state: value.prior_state,
            reuse_only: value.reuse_only,
        }
    }
}

