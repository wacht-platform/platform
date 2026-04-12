mod client;
pub mod gemini;
pub mod openai;
pub mod openrouter;

#[allow(unused_imports)]
pub(crate) mod providers {
    pub(crate) use super::gemini;
    pub(crate) use super::openai;
    pub(crate) use super::openrouter;
}

pub use client::{
    GeneratedToolCall, LlmClient, LlmRole, NativeToolDefinition, PromptCacheRequest,
    ResolvedLlm, SemanticLlmContentBlock, SemanticLlmMessage, SemanticLlmPromptConfig,
    SemanticLlmRequest, StructuredGenerationOutput, StructuredGenerationRequest,
    ToolCallGenerationOutput,
};
pub use gemini::{GeminiClient, UsageMetadata};
pub use openai::OpenAiClient;
pub use openrouter::OpenRouterClient;
