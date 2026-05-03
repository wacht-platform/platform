use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::ProjectTaskBoardItemMetadata;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskSchedule {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_id: i64,
    pub task_key: String,
    pub template_payload: serde_json::Value,
    pub state: serde_json::Value,
    pub state_version: i64,
    pub status: String,
    pub schedule_kind: String,
    pub interval_seconds: Option<i64>,
    pub next_run_at: DateTime<Utc>,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub overlap_policy: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Typed view of `ProjectTaskSchedule.template_payload`. The agent supplies
/// these fields once (when creating/updating the schedule); each fire
/// materializes a fresh `project_task_board_items` row from this snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleTemplatePayload {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: ProjectTaskBoardItemMetadata,
}


pub mod status {
    pub const ACTIVE: &str = "active";
    pub const PAUSED: &str = "paused";
    pub const COMPLETED: &str = "completed";
}

pub mod schedule_kind {
    pub const ONCE: &str = "once";
    pub const INTERVAL: &str = "interval";
}

pub mod overlap_policy {
    pub const SKIP: &str = "skip";
    pub const PARALLEL: &str = "parallel";
}
