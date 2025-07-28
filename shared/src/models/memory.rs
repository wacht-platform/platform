use chrono::{DateTime, Utc};
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

/// Memory record with enhanced importance scoring
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MemoryRecord {
    pub id: i64,
    pub content: String,
    pub embedding: Option<Vector>,
    pub memory_category: String,

    // Decay components
    pub base_temporal_score: f64,
    pub access_count: i32,
    pub first_accessed_at: DateTime<Utc>,
    pub last_accessed_at: DateTime<Utc>,

    // Learning metrics
    pub citation_count: i32,
    pub cross_context_value: f64,
    pub learning_confidence: f64,

    // Origin
    pub creation_context_id: Option<i64>,
    pub last_reinforced_at: DateTime<Utc>,

    // Importance scoring
    pub semantic_centrality: f64,
    pub uniqueness_score: f64,

    // Compression
    pub compression_level: i32,
    pub compressed_content: Option<String>,

    // Flexible decay profile
    pub context_decay_profile: Value, // JSONB

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryWithScore {
    pub memory: MemoryRecord,
    pub similarity_score: f64,
    pub decay_adjusted_score: f64,
}

/// Compression strategies
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