use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskBoard {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub actor_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub project_id: i64,
    pub title: String,
    pub status: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskBoardItem {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_id: i64,
    pub task_key: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub assigned_thread_id: Option<i64>,
    pub metadata: serde_json::Value,
    pub completed_at: Option<DateTime<Utc>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskBoardItemRelation {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub parent_board_item_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub child_board_item_id: i64,
    pub relation_type: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

pub mod task_priority {
    pub const URGENT: &str = "urgent";
    pub const HIGH: &str = "high";
    pub const NEUTRAL: &str = "neutral";
    pub const LOW: &str = "low";
}

pub mod relation_type {
    pub const CHILD_OF: &str = "child_of";
}

impl ProjectTaskBoardItem {
    pub fn typed_metadata(&self) -> ProjectTaskBoardItemMetadata {
        serde_json::from_value(self.metadata.clone()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskBoardItemEvent {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub thread_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub execution_run_id: Option<i64>,
    pub event_type: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    pub details: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

impl ProjectTaskBoardItemEvent {
    pub fn assignment_event_details(&self) -> Option<ProjectTaskBoardItemAssignmentEventDetails> {
        serde_json::from_value(self.details.clone()).ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskBoardItemAssignment {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub assignment_role: String,
    pub assignment_order: i32,
    pub status: String,
    pub instructions: Option<String>,
    pub handoff_file_path: Option<String>,
    pub metadata: serde_json::Value,
    pub result_status: Option<String>,
    pub result_summary: Option<String>,
    pub result_payload: Option<serde_json::Value>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub rejected_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ProjectTaskBoardItemAssignment {
    pub fn typed_metadata(&self) -> ProjectTaskBoardAssignmentMetadata {
        serde_json::from_value(self.metadata.clone()).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectTaskBoardItemMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectTaskBoardAssignmentMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_target: Option<ProjectTaskBoardAssignmentTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTaskBoardItemAssignmentEventDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub assignment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    pub assignment_role: String,
    pub assignment_order: i32,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_file_path: Option<String>,
    pub metadata: ProjectTaskBoardAssignmentMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTaskBoardAssignmentTarget {
    #[serde(default)]
    pub thread_id: Option<crate::FlexibleI64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responsibility: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTaskBoardAssignmentSpec {
    #[serde(flatten)]
    pub target: ProjectTaskBoardAssignmentTarget,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignment_order: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handoff_file_path: Option<String>,
}

pub mod assignment_role {
    pub const EXECUTOR: &str = "executor";
    pub const REVIEWER: &str = "reviewer";
    pub const SPECIALIST_REVIEWER: &str = "specialist_reviewer";
    pub const APPROVER: &str = "approver";
    pub const OBSERVER: &str = "observer";
}

pub mod assignment_status {
    pub const PENDING: &str = "pending";
    pub const AVAILABLE: &str = "available";
    pub const CLAIMED: &str = "claimed";
    pub const IN_PROGRESS: &str = "in_progress";
    pub const COMPLETED: &str = "completed";
    pub const REJECTED: &str = "rejected";
    pub const BLOCKED: &str = "blocked";
    pub const CANCELLED: &str = "cancelled";
}

pub mod assignment_result_status {
    pub const COMPLETED: &str = "completed";
    pub const BLOCKED: &str = "blocked";
    pub const FAILED: &str = "failed";
    pub const NEEDS_CLARIFICATION: &str = "needs_clarification";
    pub const NEEDS_REPLAN: &str = "needs_replan";
    pub const REJECTED: &str = "rejected";
    pub const CANCELLED: &str = "cancelled";
}
