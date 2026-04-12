use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// Result from hybrid search for knowledge base chunks
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct HybridSearchKbResult {
    pub document_id: i64,
    pub knowledge_base_id: i64,
    pub chunk_index: i32,
    pub content: String,
    pub document_title: Option<String>,
    pub document_description: Option<String>,
    pub vector_similarity: f64, // double precision in PostgreSQL
    pub text_rank: f64,         // double precision in PostgreSQL
    pub combined_score: f64,    // double precision in PostgreSQL
}

/// Result from full-text search
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct FullTextSearchResult {
    pub document_id: i64,
    pub knowledge_base_id: i64,
    pub chunk_index: i32,
    pub content: String,
    pub text_rank: f64,
    pub document_title: Option<String>,
    pub document_description: Option<String>,
}
