use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskSchedule {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub template_board_item_id: i64,
    pub status: String,
    pub schedule_kind: String,
    pub interval_seconds: Option<i64>,
    pub next_run_at: DateTime<Utc>,
    pub last_enqueued_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
