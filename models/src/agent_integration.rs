use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum IntegrationType {
    Teams,
    Slack,
    WhatsApp,
    Discord,
    ClickUp,
}

impl std::fmt::Display for IntegrationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IntegrationType::Teams => write!(f, "teams"),
            IntegrationType::Slack => write!(f, "slack"),
            IntegrationType::WhatsApp => write!(f, "whatsapp"),
            IntegrationType::Discord => write!(f, "discord"),
            IntegrationType::ClickUp => write!(f, "clickup"),
        }
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
