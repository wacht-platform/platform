use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentThread {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub actor_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub project_id: i64,
    pub title: String,
    pub thread_kind: String,
    pub thread_visibility: String,
    pub thread_purpose: String,
    pub responsibility: Option<String>,
    pub reusable: bool,
    pub accepts_assignments: bool,
    pub capability_tags: Vec<String>,
    pub status: String,
    pub system_instructions: Option<String>,
    pub last_activity_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub execution_state: Option<serde_json::Value>,
    pub next_event_sequence: i64,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

pub mod visibility {
    pub const USER_FACING: &str = "user_facing";
    pub const INTERNAL: &str = "internal";
}

pub mod purpose {
    pub const CONVERSATION: &str = "conversation";
    pub const COORDINATOR: &str = "coordinator";
    pub const EXECUTION: &str = "execution";
    pub const REVIEW: &str = "review";
}
