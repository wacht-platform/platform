use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct PulseTransaction {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub billing_account_id: i64,
    pub amount_pulse_cents: i64,
    pub transaction_type: PulseTransactionType,
    pub reference_id: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PulseTransactionType {
    Purchase,
    UsageSms,
    UsageAi,
    Refund,
    Adjustment,
}

impl std::fmt::Display for PulseTransactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Purchase => "purchase",
            Self::UsageSms => "usage_sms",
            Self::UsageAi => "usage_ai",
            Self::Refund => "refund",
            Self::Adjustment => "adjustment",
        };
        write!(f, "{}", s)
    }
}
