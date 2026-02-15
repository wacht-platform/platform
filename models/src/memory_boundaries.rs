use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

/// Memory boundaries configuration
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MemoryBoundaries {
    pub context_id: i64,
    pub max_conversations: i32,
    pub max_memories_per_category: Value,
    pub compression_threshold_days: i32,
    pub eviction_threshold_score: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
