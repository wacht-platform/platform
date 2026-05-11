use async_trait::async_trait;
use common::error::AppError;
use serde_json::Value;

use super::client::{
    NativeToolDefinition, PromptCacheRequest, SemanticLlmRequest, StructuredGenerationOutput,
    TextGenerationOutput, ToolCallGenerationOutput,
};

#[async_trait]
pub trait LlmProvider: Send + Sync + std::fmt::Debug {
    fn provider_label(&self) -> &'static str;

    async fn generate_structured(
        &self,
        prompt: SemanticLlmRequest,
        cache: Option<PromptCacheRequest>,
    ) -> Result<StructuredGenerationOutput<Value>, AppError>;

    async fn generate_tool_calls(
        &self,
        prompt: SemanticLlmRequest,
        tools: Vec<NativeToolDefinition>,
        cache: Option<PromptCacheRequest>,
    ) -> Result<ToolCallGenerationOutput, AppError>;

    async fn generate_text(
        &self,
        prompt: SemanticLlmRequest,
    ) -> Result<TextGenerationOutput, AppError>;
}
