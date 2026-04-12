use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentExecutionRecoveryEntry {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub thread_event_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub execution_run_id: Option<i64>,
    pub reason_code: String,
    pub reason_detail: serde_json::Value,
    pub status: String,
    pub retry_count: i32,
    pub last_recovery_attempt_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub mod reason_code {
    pub const CLAIMED_EVENT_STALE: &str = "claimed_event_stale";
    pub const EXECUTION_RUN_STUCK: &str = "execution_run_stuck";
    pub const THREAD_RUNNING_WITHOUT_LOCK: &str = "thread_running_without_lock";
    pub const CLAIMED_EVENT_WITHOUT_LIVE_RUN: &str = "claimed_event_without_live_run";
    pub const RUN_FAILED_WITHOUT_EVENT_RESOLUTION: &str = "run_failed_without_event_resolution";
}

pub mod recovery_status {
    pub const OPEN: &str = "open";
    pub const REQUEUED: &str = "requeued";
    pub const RELEASED: &str = "released";
    pub const FAILED_TERMINAL: &str = "failed_terminal";
    pub const IGNORED: &str = "ignored";
    pub const RESOLVED: &str = "resolved";
}
