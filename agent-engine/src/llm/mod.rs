mod client;
pub mod gemini;
pub mod openai;
pub mod openrouter;
mod provider;
pub(crate) mod usage;

#[allow(unused_imports)]
pub(crate) mod providers {
    pub(crate) use super::gemini;
    pub(crate) use super::openai;
    pub(crate) use super::openrouter;
}

pub use client::{
    GeneratedToolCall, LlmRole, NativeToolDefinition, PromptCacheRequest, ResolvedLlm,
    SemanticLlmContentBlock, SemanticLlmMessage, SemanticLlmPromptConfig, SemanticLlmRequest,
    StructuredGenerationOutput, TextGenerationOutput, ToolCallGenerationOutput,
};
pub use gemini::{GeminiClient, UsageMetadata};
pub use openai::OpenAiClient;
pub use openrouter::OpenRouterClient;
pub use provider::LlmProvider;
