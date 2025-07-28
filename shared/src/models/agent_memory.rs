use serde::{Deserialize, Serialize};

use super::conversation::{ConversationRecord, CitationType};
use super::memory::{MemoryRecord, MemoryWithScore};

/// Immediate context for Phase 1 retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImmediateContext {
    pub memories: Vec<MemoryRecord>,
    pub conversations: Vec<ConversationRecord>,
}

/// Refined context for Phase 2 retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefinedContext {
    pub relevant_memories: Vec<MemoryWithScore>,
    pub relevant_conversations: Vec<ConversationRecord>,
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

