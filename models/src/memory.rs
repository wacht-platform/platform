use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ConversationRecord;

pub mod scope {
    pub const ACTOR: &str = "actor";
    pub const PROJECT: &str = "project";
    pub const THREAD: &str = "thread";
}

/// Memory record used for retrieval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub actor_id: Option<i64>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub project_id: Option<i64>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub thread_id: Option<i64>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub execution_run_id: Option<i64>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
    pub owner_agent_id: Option<i64>,
    #[serde(default, with = "crate::utils::serde::i64_as_string_option")]
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
    #[serde(default)]
    pub routing_events: Vec<TaskRoutingEvent>,
    #[serde(default)]
    pub task_thread_meta: Vec<TaskThreadMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRoutingEvent {
    pub id: i64,
    pub coordinator_thread_id: Option<i64>,
    pub routing_reason: Option<String>,
    pub summary: Option<String>,
    pub note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskThreadMeta {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub title: String,
    pub thread_purpose: String,
}
