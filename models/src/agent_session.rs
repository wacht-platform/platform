use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "varchar", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionIdentifier {
    Signin,
    Static,
}

impl std::fmt::Display for AgentSessionIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentSessionIdentifier::Signin => write!(f, "signin"),
            AgentSessionIdentifier::Static => write!(f, "static"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentSession {
    pub id: i64,
    pub session_id: i64,
    pub deployment_id: i64,
    pub identifier: String,
    pub context_group: String,
    pub agent_ids: Vec<i64>,
    pub expires_at: Option<DateTime<Utc>>,
}

impl AgentSession {
    pub fn has_agent_access(&self, agent_id: i64) -> bool {
        self.agent_ids.contains(&agent_id)
    }

    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            expires_at < Utc::now()
        } else {
            false
        }
    }

    pub fn is_active(&self) -> bool {
        self.deleted_at.is_none() && !self.is_expired()
    }
}
