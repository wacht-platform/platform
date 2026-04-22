use common::error::AppError;
use models::PromptCacheState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    gemini::{ExplicitCacheRequest, GeminiClient},
    openai::OpenAiClient,
    openrouter::OpenRouterClient,
    UsageMetadata,
};

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

#[derive(Debug, Clone)]
pub struct StructuredGenerationRequest {
    pub request_body: String,
    pub cache: Option<PromptCacheRequest>,
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuredGenerationOutput<T> {
    pub value: T,
    pub usage_metadata: Option<UsageMetadata>,
    pub cache_state: Option<PromptCacheState>,
}

#[derive(Debug, Clone)]
pub enum LlmClient {
    Gemini(GeminiClient),
    OpenAi(OpenAiClient),
    OpenRouter(OpenRouterClient),
}

#[derive(Debug, Clone)]
pub struct ResolvedLlm {
    client: LlmClient,
    model_name: String,
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

    pub async fn generate_structured_from_prompt<T>(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<StructuredGenerationOutput<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        self.client
            .generate_structured_from_prompt(prompt, cache)
            .await
    }

    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        self.client.generate_tool_calls(prompt, tools).await
    }
}

impl LlmClient {
    pub async fn generate_structured_from_prompt<T>(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<StructuredGenerationOutput<T>, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        match self {
            Self::Gemini(client) => {
                let output = client
                    .generate_structured_content_with_usage_and_cache::<T>(
                        serialize_gemini_request(&prompt)?,
                        cache.map(Into::into),
                    )
                    .await?;
                Ok(StructuredGenerationOutput {
                    value: output.value,
                    usage_metadata: output.usage_metadata,
                    cache_state: output.cache_state,
                })
            }
            Self::OpenAi(client) => client.generate_structured_from_prompt(prompt, cache).await,
            Self::OpenRouter(client) => client.generate_structured_from_prompt(prompt, cache).await,
        }
    }

    pub async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
    ) -> Result<ToolCallGenerationOutput, AppError> {
        match self {
            Self::Gemini(client) => client.generate_tool_calls(prompt, tools).await,
            Self::OpenAi(client) => client.generate_tool_calls(prompt, tools).await,
            Self::OpenRouter(client) => client.generate_tool_calls(prompt, tools).await,
        }
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

fn serialize_gemini_request(prompt: &SemanticLlmRequest) -> Result<String, AppError> {
    let contents = prompt
        .messages
        .iter()
        .map(|message| {
            let parts = message
                .content_blocks
                .iter()
                .map(|block| match block {
                    SemanticLlmContentBlock::Text { text } => {
                        json!({ "text": text })
                    }
                    SemanticLlmContentBlock::InlineData { mime_type, data } => {
                        json!({ "inline_data": { "mime_type": mime_type, "data": data } })
                    }
                })
                .collect::<Vec<_>>();
            json!({
                "role": message.role,
                "parts": parts,
            })
        })
        .collect::<Vec<_>>();

    let mut generation_config = serde_json::Map::new();
    generation_config.insert("responseMimeType".to_string(), json!("application/json"));
    generation_config.insert(
        "responseSchema".to_string(),
        prompt.response_json_schema.clone(),
    );
    if let Some(temperature) = prompt.temperature {
        generation_config.insert("temperature".to_string(), json!(temperature));
    }
    if let Some(max_output_tokens) = prompt.max_output_tokens {
        generation_config.insert("maxOutputTokens".to_string(), json!(max_output_tokens));
    }
    if let Some(reasoning_effort) = prompt.reasoning_effort.as_ref() {
        generation_config.insert(
            "thinkingConfig".to_string(),
            json!({ "thinkingLevel": reasoning_effort }),
        );
    }

    serde_json::to_string(&json!({
        "system_instruction": {
            "parts": [{ "text": prompt.system_prompt }]
        },
        "contents": contents,
        "generationConfig": generation_config,
    }))
    .map_err(|e| AppError::Internal(format!("Failed to serialize LLM request: {e}")))
}
