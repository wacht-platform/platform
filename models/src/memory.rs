use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ConversationRecord;

pub mod scope {
    pub const ACTOR: &str = "actor";
    pub const PROJECT: &str = "project";
    pub const THREAD: &str = "thread";
    pub const AGENT: &str = "agent";
}

/// Memory record used for retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: i64,
    pub deployment_id: i64,
    pub actor_id: Option<i64>,
    pub project_id: Option<i64>,
    pub thread_id: Option<i64>,
    pub execution_run_id: Option<i64>,
    pub owner_agent_id: Option<i64>,
    pub recorded_by_agent_id: Option<i64>,
    pub memory_scope: String,
    pub content: String,
    #[serde(skip)]
    pub embedding: Option<Vec<f32>>,
    pub memory_category: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl MemoryRecord {
    pub fn is_thread_scoped(&self) -> bool {
        self.memory_scope == scope::THREAD
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImmediateContext {
    pub memories: Vec<MemoryRecord>,
    pub conversations: Vec<ConversationRecord>,
}
