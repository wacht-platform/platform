use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TaskHandoffSummary {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub assignment_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub execution_run_id: Option<i64>,
    pub assignment_role: String,
    pub outcome: String,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub artifacts: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub blockers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub next_actions: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
