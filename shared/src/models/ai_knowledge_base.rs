use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct AiKnowledgeBase {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
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
    pub description: Option<String>,
    pub file_name: String,
    pub file_size: i64,
    pub file_type: String,
    pub file_url: String,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub knowledge_base_id: i64,
    pub processing_metadata: Option<serde_json::Value>,
}
