use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Admin-safe view of a user's passkey. Excludes credential_id / public_key /
/// aaguid since exposing those over an admin REST surface adds no value and
/// only widens blast radius if the response leaks.
#[derive(Serialize, Deserialize, Clone)]
pub struct UserPasskey {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub user_id: i64,
    pub name: String,
    pub sign_count: i32,
    pub transports: Option<Vec<String>>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub backed_up: Option<bool>,
    pub device_type: Option<String>,
}
