use chrono::{DateTime, Utc};
use pgvector::HalfVector;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(FromRow, Clone)]
pub struct KnowledgeBaseDocumentChunk {
    pub document_id: i64,
    pub knowledge_base_id: i64,
    pub deployment_id: i64,
    pub chunk_index: i32,
    pub content: String,
    pub embedding: HalfVector,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(FromRow, Serialize, Deserialize, Clone, Debug)]
pub struct DocumentChunkSearchResult {
    pub document_id: i64,
    pub knowledge_base_id: i64,
    pub content: String,
    pub score: f64,
    pub chunk_index: i32,
    pub document_title: Option<String>,
    pub document_description: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiKnowledgeBase {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiKnowledgeBaseWithDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub configuration: serde_json::Value,
    pub documents_count: i64,
    pub total_size: i64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AiKnowledgeBaseDocument {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub file_url: String,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub knowledge_base_id: i64,
    pub processing_metadata: Option<serde_json::Value>,
}
