use chrono::{DateTime, Utc};
use pgvector::HalfVector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

/// Memory record with enhanced importance scoring
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MemoryRecord {
    pub id: i64,
    pub content: String,
    #[serde(skip)]
    pub embedding: Option<HalfVector>,
    pub memory_category: String,
    pub base_temporal_score: f64,
    pub access_count: i32,
    pub first_accessed_at: DateTime<Utc>,
    pub last_accessed_at: DateTime<Utc>,
    pub creation_context_id: Option<i64>,
    pub last_reinforced_at: DateTime<Utc>,
    pub semantic_centrality: f64,
    pub uniqueness_score: f64,
    pub compression_level: i32,
    pub compressed_content: Option<String>,
    pub context_decay_profile: Value, // JSONB
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressionLevel {
    None = 0,
    Summary = 1,
    Keywords = 2,
}

impl From<i32> for CompressionLevel {
    fn from(value: i32) -> Self {
        match value {
            1 => CompressionLevel::Summary,
            2 => CompressionLevel::Keywords,
            _ => CompressionLevel::None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConsolidationCandidate {
    pub primary_id: i64,
    pub similar_ids: Vec<i64>,
    pub similarity_scores: Vec<f64>,
    pub suggested_content: String,
    pub suggested_category: String,
}

impl MemoryRecord {
    pub fn effective_content(&self) -> &str {
        match self.compression_level {
            0 => &self.content,
            _ => self.compressed_content.as_deref().unwrap_or(&self.content),
        }
    }

    pub fn get_context_decay_modifier(&self, context_id: i64) -> f64 {
        self.context_decay_profile
            .get(&context_id.to_string())
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0)
    }
}
