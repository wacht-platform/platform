use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentOrgSettings {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deployment_id: i64,
    pub enabled: bool,
    pub ip_allowlist_enabled: bool,
    pub max_allowed_members: i64,
    pub allow_deletion: bool,
    pub custom_role_enabled: bool,
    pub default_role: String,
}

impl Default for DeploymentOrgSettings {
    fn default() -> Self {
        Self {
            id: 0,
            created_at: None,
            updated_at: None,
            deployment_id: 0,
            enabled: true,
            ip_allowlist_enabled: false,
            max_allowed_members: 100,
            allow_deletion: true,
            custom_role_enabled: true,
            default_role: "member".to_string(),
        }
    }
}
