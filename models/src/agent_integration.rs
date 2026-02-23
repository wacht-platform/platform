use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum IntegrationType {
    Teams,
    ClickUp,
    Mcp,
}

impl IntegrationType {
    pub const fn as_str(self) -> &'static str {
        match self {
            IntegrationType::Teams => "teams",
            IntegrationType::ClickUp => "clickup",
            IntegrationType::Mcp => "mcp",
        }
    }
}

impl FromStr for IntegrationType {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "teams" => Ok(IntegrationType::Teams),
            "clickup" => Ok(IntegrationType::ClickUp),
            "mcp" => Ok(IntegrationType::Mcp),
            _ => Err(format!("Unknown integration type: {}", value)),
        }
    }
}

impl std::fmt::Display for IntegrationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AgentIntegration {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub agent_id: i64,
    pub integration_type: IntegrationType,
    pub name: String,
    pub config: serde_json::Value,
}
