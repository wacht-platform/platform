use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApprovalPolicy {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub actor_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub project_id: Option<i64>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub tool_id: i64,
    pub policy_scope: String,
    pub decision: String,
    pub metadata: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApprovalGrant {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub policy_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub actor_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub project_id: Option<i64>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub thread_id: Option<i64>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub tool_id: i64,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub granted_by_message_id: Option<i64>,
    pub grant_scope: String,
    pub status: String,
    pub granted_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub consumed_at: Option<DateTime<Utc>>,
    #[serde(
        serialize_with = "crate::utils::serde::serialize_option_i64_as_string",
        skip_serializing_if = "Option::is_none"
    )]
    pub consumed_by_run_id: Option<i64>,
    pub metadata: serde_json::Value,
}

pub mod policy_scope {
    pub const ACTOR: &str = "actor";
    pub const PROJECT: &str = "project";
}

pub mod decision {
    pub const ALLOW: &str = "allow";
    pub const DENY: &str = "deny";
    pub const REQUIRE_APPROVAL: &str = "require_approval";
}

pub mod grant_scope {
    pub const ONCE: &str = "once";
    pub const THREAD: &str = "thread";
    pub const PROJECT: &str = "project";
    pub const ACTOR: &str = "actor";
}

pub mod grant_status {
    pub const ACTIVE: &str = "active";
    pub const CONSUMED: &str = "consumed";
    pub const REVOKED: &str = "revoked";
    pub const EXPIRED: &str = "expired";
}
