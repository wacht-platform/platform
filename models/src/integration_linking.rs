use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Temporary code for linking a Wacht user to an external integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationLinkCode {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub context_group: String,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub agent_id: i64,
    pub integration_type: String,
    pub code: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
}

/// Active connection between a context group and an external integration identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveAgentIntegration {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub context_group: String,  // Flexible: could be user_id, org_id, custom group
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub integration_id: i64,
    pub external_id: String,
    pub connection_metadata: Option<serde_json::Value>,  // tenantId stored here if needed
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}
