use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subscription {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub user_id: Option<i64>,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub organization_id: Option<i64>,
    pub chargebee_customer_id: String,
    pub chargebee_subscription_id: String,
    pub status: String, // 'active', 'cancelled', 'past_due', 'trialing'
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}