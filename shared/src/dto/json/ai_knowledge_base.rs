use serde::{Deserialize, Serialize};

use crate::{models::AiKnowledgeBaseWithDetails, services::qdrant::SearchResult};

// Knowledge Base CRUD Models
#[derive(Debug, Deserialize)]
pub struct CreateKnowledgeBaseRequest {
    pub name: String,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateKnowledgeBaseRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
}

// Document Upload Models
#[derive(Debug, Deserialize)]
pub struct UploadUrlRequest {
    pub title: String,
    pub description: Option<String>,
    pub url: String,
}

// Knowledge Base Response Models
#[derive(Debug, Serialize)]
pub struct KnowledgeBaseResponse {
    pub data: Vec<AiKnowledgeBaseWithDetails>,
    pub has_more: bool,
}

// Document Query Models
#[derive(Debug, Deserialize)]
pub struct GetDocumentsQuery {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

// Search Models
#[derive(Debug, Deserialize)]
pub struct SearchKnowledgeBaseQuery {
    pub query: String,
    pub limit: Option<u64>,
    pub knowledge_base_id: Option<i64>,
    pub sort_by_relevance: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct SearchKnowledgeBaseResponse {
    pub results: Vec<KnowledgeBaseSearchResult>,
    pub total_results: usize,
    pub query: String,
}

#[derive(Debug, Serialize)]
pub struct KnowledgeBaseSearchResult {
    pub id: String,
    pub content: String,
    pub score: f32,
    pub document_id: Option<String>,
    pub knowledge_base_id: Option<String>,
    pub title: Option<String>,
    pub file_type: Option<String>,
    pub chunk_index: Option<i64>,
}

impl From<SearchResult> for KnowledgeBaseSearchResult {
    fn from(result: SearchResult) -> Self {
        Self {
            id: result.id.to_string(),
            content: result.content,
            score: result.score,
            document_id: result
                .metadata
                .get("document_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            knowledge_base_id: result
                .metadata
                .get("knowledge_base_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            title: result
                .metadata
                .get("title")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            file_type: result
                .metadata
                .get("file_type")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            chunk_index: result.metadata.get("chunk_index").and_then(|v| v.as_i64()),
        }
    }
}
