use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserAuthenticator {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub user_id: i64,
    #[serde(skip_serializing)]
    pub totp_secret: String,
    pub otp_url: Option<String>,
}
