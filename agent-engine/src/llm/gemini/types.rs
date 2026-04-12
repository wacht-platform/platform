use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const GEMINI_STRUCTURED_OUTPUT_TRUNCATED_MARKER: &str = "gemini_structured_output_truncated";

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
    #[serde(default)]
    pub text: Option<String>,
    #[serde(rename = "functionCall", default)]
    pub function_call: Option<GeminiFunctionCall>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GeminiFunctionCall {
    pub name: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GroundingMetadata {
    #[serde(rename = "webSearchQueries", default)]
    pub web_search_queries: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ModalityTokenCount {
    pub modality: String,
    #[serde(rename = "tokenCount")]
    pub token_count: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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

#[derive(Debug, Clone)]
pub struct StructuredContentOutput<T> {
    pub value: T,
    pub usage_metadata: Option<UsageMetadata>,
    pub cache_state: Option<models::PromptCacheState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExplicitCacheRequest {
    pub cache_key: String,
    pub ttl_secs: i64,
    pub live_tail_count: usize,
    pub prior_state: Option<models::PromptCacheState>,
    #[serde(default)]
    pub reuse_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CachedContentUsageMetadata {
    #[serde(rename = "promptTokenCount", default)]
    pub(crate) prompt_token_count: Option<u32>,
    #[serde(rename = "cachedContentTokenCount", default)]
    pub(crate) cached_content_token_count: Option<u32>,
    #[serde(rename = "candidatesTokenCount", default)]
    pub(crate) candidates_token_count: Option<u32>,
    #[serde(rename = "totalTokenCount", default)]
    pub(crate) total_token_count: Option<u32>,
    #[serde(rename = "thoughtsTokenCount", default)]
    pub(crate) thoughts_token_count: Option<u32>,
    #[serde(rename = "toolUsePromptTokenCount", default)]
    pub(crate) tool_use_prompt_token_count: Option<u32>,
    #[serde(rename = "promptTokensDetails", default)]
    pub(crate) prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
    #[serde(rename = "cacheTokensDetails", default)]
    pub(crate) cache_tokens_details: Option<Vec<ModalityTokenCount>>,
    #[serde(rename = "candidatesTokensDetails", default)]
    pub(crate) candidates_tokens_details: Option<Vec<ModalityTokenCount>>,
    #[serde(rename = "toolUsePromptTokensDetails", default)]
    pub(crate) tool_use_prompt_tokens_details: Option<Vec<ModalityTokenCount>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CachedContentResponse {
    pub(crate) name: String,
    #[serde(rename = "expireTime", default)]
    pub(crate) expire_time: Option<String>,
    #[serde(rename = "usageMetadata", default)]
    pub(crate) usage_metadata: Option<CachedContentUsageMetadata>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExplicitCachePlan {
    pub(crate) full_cache_payload: Value,
    pub(crate) send_request_payload: Value,
    pub(crate) prefix_signature: String,
    pub(crate) cached_contents_signature: String,
    pub(crate) cached_content_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedGenerateRequest {
    pub(crate) request_body: String,
    pub(crate) cache_plan: Option<ExplicitCachePlan>,
}
