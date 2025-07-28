use serde::{Deserialize, Serialize};

// DTO types for context orchestrator

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchDerivation {
    pub search_query: String,
    pub search_scope: SearchScope,
    pub filters: LLMFilters,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_documents_params: Option<ListDocumentsParams>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_document_params: Option<ReadDocumentParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMFilters {
    pub max_results: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boost_keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_range: Option<String>,
    pub search_mode: LLMSearchMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge_base_ids: Option<Vec<String>>, // String to handle snowflake IDs
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LLMSearchMode {
    Semantic,
    Keyword,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDocumentsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge_base_ids: Option<Vec<String>>, // Multiple KB IDs for simultaneous pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_filter: Option<String>,
    pub page: i32, // Page number starting from 1
    pub limit: i32, // Number of documents per page
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadDocumentParams {
    pub document_id: String, // String to handle snowflake IDs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_range: Option<ChunkRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keywords: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkRange {
    pub start: i32,
    pub end: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchScope {
    KnowledgeBase,
    Experience,
    Universal,
    ListKnowledgeBaseDocuments,
    ReadKnowledgeBaseDocuments,
    GatheredContext,
}