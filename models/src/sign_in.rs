use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct SignIn {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub session_id: i64,
    pub user_id: Option<i64>,
    pub active_organization_membership_id: Option<i64>,
    pub active_workspace_membership_id: Option<i64>,
    pub expires_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub ip_address: String,
    pub browser: String,
    pub device: String,
    pub city: String,
    pub region: String,
    pub region_code: String,
    pub country: String,
    pub country_code: String,
}
