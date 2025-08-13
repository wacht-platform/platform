use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::SocialConnectionProvider;

#[derive(Serialize, Deserialize, Clone)]
pub struct SocialConnection {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub user_id: i64,
    pub user_email_address_id: i64,
    pub provider: SocialConnectionProvider,
    pub email_address: String,
    pub access_token: String,
    pub refresh_token: String,
}
