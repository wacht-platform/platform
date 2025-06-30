use serde::{Deserialize, Serialize};

use crate::{models::AiKnowledgeBaseWithDetails, services::clickhouse::DocumentSearchResult};

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

impl From<DocumentSearchResult> for KnowledgeBaseSearchResult {
    fn from(result: DocumentSearchResult) -> Self {
        Self {
            id: result.id.to_string(),
            content: result.content,
            score: result.score,
            document_id: Some(result.document_id.to_string()),
            knowledge_base_id: Some(result.knowledge_base_id.to_string()),
            title: None, // Not available in DocumentSearchResult
            file_type: None, // Not available in DocumentSearchResult
            chunk_index: Some(result.chunk_index as i64),
        }
    }
}
