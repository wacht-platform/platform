pub use super::tool_calls::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AbortDirective {
    pub outcome: AbortOutcome,
    pub reason: String,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum AbortOutcome {
    Blocked,
    ReturnToCoordinator,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum LocalKnowledgeSearchType {
    Semantic,
    Keyword,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemorySearchApproach {
    Semantic,
    FullText,
    Hybrid,
}

impl Default for MemorySearchApproach {
    fn default() -> Self {
        Self::Semantic
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SearchPattern {
    Troubleshooting,
    Implementation,
    Analysis,
    Historical,
    Exploration,
    Verification,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SearchDepth {
    Shallow,
    Moderate,
    Deep,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemorySource {
    Thread,
    Project,
    Actor,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ApprovalRequestData {
    #[serde(default)]
    pub description: String,
    pub tool_names: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageData {
    pub mime_type: String,
    pub data: String,
}

/// Generic file data for any file type upload
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileData {
    pub filename: String,
    pub mime_type: String,
    pub data: String, // base64 encoded
}

#[derive(Clone, Debug)]
pub struct ConverseRequest {
    pub conversation_id: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextHints {
    pub recommended_files: Vec<RecommendedFile>,
    pub search_summary: String,
    pub search_conclusion: SearchConclusion,
    pub search_terms_used: Vec<String>,
    pub knowledge_bases_searched: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extracted_output: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextChunkMatch {
    pub path: String,
    pub document_title: String,
    pub document_id: String,
    pub knowledge_base_id: String,
    pub chunk_index: i32,
    pub relevance_score: f32,
    pub excerpt: String,
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecommendedFile {
    pub path: String,
    pub document_title: String,
    pub relevance_score: f32,
    pub reason: String,
    pub sample_text: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SearchConclusion {
    FoundRelevant,
    PartialMatch,
    NothingFound,
    NeedsMoreContext,
}

/// Response from spawning a child thread
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnThreadResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub status: String,
    pub message: String,
}
