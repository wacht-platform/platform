use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Search mode for hybrid search
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchMode {
    /// Semantic search only using vector embeddings
    Vector,
    /// Full-text search only using PostgreSQL text search
    FullText,
    /// Hybrid search combining vector and full-text with weights
    Hybrid {
        vector_weight: f32,
        text_weight: f32,
    },
}

/// Filters for context searches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextFilters {
    pub max_results: usize,
    pub time_range: Option<TimeRange>,
    pub search_mode: SearchMode,
    pub boost_keywords: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Result from context engine search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSearchResult {
    pub source: ContextSource,
    pub content: String,
    pub relevance_score: f64,
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSource {
    KnowledgeBase { kb_id: i64, document_id: i64 },
    Memory { memory_id: i64, category: String },
    Conversation { conversation_id: i64 },
}

impl Default for ContextFilters {
    fn default() -> Self {
        Self {
            max_results: 10,
            time_range: None,
            search_mode: SearchMode::Hybrid {
                vector_weight: 0.7,
                text_weight: 0.3,
            },
            boost_keywords: None,
        }
    }
}

impl Default for SearchMode {
    fn default() -> Self {
        SearchMode::Hybrid {
            vector_weight: 0.7,
            text_weight: 0.3,
        }
    }
}