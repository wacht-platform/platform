use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;
use pgvector::Vector;

/// Enhanced citation with rich metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedCitation {
    pub item_id: i64,
    pub item_type: CitationType,
    pub relevance_score: f64,
    pub usefulness_score: f64,
    pub confidence: f64,
    pub usage_type: UsageType,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationType {
    Memory,
    Conversation,
    KnowledgeBase,
    DynamicContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageType {
    DirectQuote,
    Paraphrase,
    Inspiration,
    Background,
}

/// Conversation record with compression support
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ConversationRecord {
    pub id: i64,
    pub context_id: i64,
    pub timestamp: DateTime<Utc>,
    pub content: String,
    pub embedding: Option<Vector>,
    pub message_type: String,
    
    // Decay components
    pub base_temporal_score: f64,
    pub access_count: i32,
    pub first_accessed_at: DateTime<Utc>,
    pub last_accessed_at: DateTime<Utc>,
    
    // Learning metrics
    pub citation_count: i32,
    pub relevance_score: f64,
    pub usefulness_score: f64,
    
    // Compression
    pub compression_level: i32,
    pub compressed_content: Option<String>,
    
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Memory record with enhanced importance scoring
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MemoryRecordV2 {
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

/// Memory boundaries configuration
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MemoryBoundaries {
    pub context_id: i64,
    pub max_conversations: i32,
    pub max_memories_per_category: Value, // JSONB
    pub compression_threshold_days: i32,
    pub eviction_threshold_score: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Immediate context for Phase 1 retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImmediateContext {
    pub memories: Vec<MemoryRecordV2>,
    pub conversations: Vec<ConversationRecord>,
}

/// Refined context for Phase 2 retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinedContext {
    pub relevant_memories: Vec<MemoryWithScore>,
    pub relevant_conversations: Vec<ConversationWithScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryWithScore {
    pub memory: MemoryRecordV2,
    pub similarity_score: f64,
    pub decay_adjusted_score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationWithScore {
    pub conversation: ConversationRecord,
    pub similarity_score: f64,
    pub decay_adjusted_score: f64,
}

/// Citation update request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CitationUpdate {
    pub item_id: i64,
    pub item_type: CitationType,
    pub relevance_delta: f64,
    pub usefulness_delta: f64,
    pub was_helpful: bool,
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

/// Memory consolidation candidate
#[derive(Debug, Clone)]
pub struct ConsolidationCandidate {
    pub primary_id: i64,
    pub similar_ids: Vec<i64>,
    pub similarity_scores: Vec<f64>,
    pub suggested_content: String,
    pub suggested_category: String,
}

impl ConversationRecord {
    /// Get effective content based on compression level
    pub fn effective_content(&self) -> &str {
        match self.compression_level {
            0 => &self.content,
            _ => self.compressed_content.as_deref().unwrap_or(&self.content),
        }
    }
    
    /// Check if conversation should be compressed
    pub fn should_compress(&self, threshold_days: i32) -> bool {
        let days_old = (Utc::now() - self.created_at).num_days();
        days_old > threshold_days as i64 && self.compression_level == 0
    }
}

impl MemoryRecordV2 {
    /// Get effective content based on compression level
    pub fn effective_content(&self) -> &str {
        match self.compression_level {
            0 => &self.content,
            _ => self.compressed_content.as_deref().unwrap_or(&self.content),
        }
    }
    
    /// Get decay modifier for specific context
    pub fn get_context_decay_modifier(&self, context_id: i64) -> f64 {
        self.context_decay_profile
            .get(&context_id.to_string())
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0)
    }
}

// Temporary conversion to support old code
impl From<MemoryRecordV2> for crate::models::MemoryEntry {
    fn from(record: MemoryRecordV2) -> Self {
        use crate::models::MemoryType;
        
        let memory_type = match record.memory_category.as_str() {
            "procedural" => MemoryType::Procedural,
            "semantic" => MemoryType::Semantic,
            "episodic" => MemoryType::Episodic,
            _ => MemoryType::Working,
        };

        Self {
            id: record.id,
            memory_type,
            content: record.effective_content().to_string(),
            metadata: std::collections::HashMap::new(),
            importance: record.learning_confidence,
            created_at: record.created_at,
            last_accessed: record.last_accessed_at,
            access_count: record.access_count as u32,
            embedding: record.embedding.map(|e| e.into()).unwrap_or_default(),
        }
    }
}