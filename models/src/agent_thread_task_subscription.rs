use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskSubscriptionEventKind {
    Completed,
    Blocked,
    Cancelled,
}

impl TaskSubscriptionEventKind {
    pub fn from_status(status: &str) -> Option<Self> {
        match status {
            "completed" => Some(Self::Completed),
            "blocked" => Some(Self::Blocked),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn defaults() -> Vec<Self> {
        vec![Self::Completed, Self::Blocked, Self::Cancelled]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentThreadTaskSubscription {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub thread_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_item_id: i64,
    pub event_kinds: Vec<TaskSubscriptionEventKind>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
