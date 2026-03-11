use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ExecutionTaskGraph {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub context_id: i64,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ExecutionTaskNode {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub graph_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub retry_count: i32,
    pub max_retries: i32,
    pub input: Option<serde_json::Value>,
    pub output: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ExecutionTaskEdge {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub graph_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub from_node_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub to_node_id: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTaskGraphSummary {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub graph_id: i64,
    pub graph_status: String,
    pub total_nodes: i64,
    pub pending_nodes: i64,
    pub ready_nodes: i64,
    pub in_progress_nodes: i64,
    pub completed_nodes: i64,
    pub failed_nodes: i64,
    pub cancelled_nodes: i64,
    pub progress_percent: f64,
}

pub mod status {
    pub const GRAPH_ACTIVE: &str = "active";
    pub const GRAPH_COMPLETED: &str = "completed";
    pub const GRAPH_FAILED: &str = "failed";
    pub const GRAPH_CANCELLED: &str = "cancelled";

    pub const NODE_PENDING: &str = "pending";
    pub const NODE_IN_PROGRESS: &str = "in_progress";
    pub const NODE_COMPLETED: &str = "completed";
    pub const NODE_FAILED: &str = "failed";
    pub const NODE_CANCELLED: &str = "cancelled";
}
