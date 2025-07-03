use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeploymentWaitlistUser {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub email_address: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
}
