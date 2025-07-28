use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BillingEventStatus {
    Pending,
    Processed,
    Failed,
}

impl std::fmt::Display for BillingEventStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BillingEventStatus::Pending => write!(f, "pending"),
            BillingEventStatus::Processed => write!(f, "processed"),
            BillingEventStatus::Failed => write!(f, "failed"),
        }
    }
}

impl std::str::FromStr for BillingEventStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(BillingEventStatus::Pending),
            "processed" => Ok(BillingEventStatus::Processed),
            "failed" => Ok(BillingEventStatus::Failed),
            _ => Err(format!("Invalid billing event status: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for BillingEventStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for BillingEventStatus {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for BillingEventStatus {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentBillingEvent {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub user_id: Option<i64>,
    pub event_type: String,
    pub stripe_event_id: Option<String>,
    pub resource_type: String,
    pub resource_id: Option<String>,
    pub status: BillingEventStatus,
    pub error_message: Option<String>,
    pub event_data: Value,
    pub processed_at: Option<DateTime<Utc>>,
}

impl DeploymentBillingEvent {
    pub fn is_stripe_event(&self) -> bool {
        self.stripe_event_id.is_some()
    }

    pub fn is_processed(&self) -> bool {
        self.status == BillingEventStatus::Processed
    }

    pub fn is_failed(&self) -> bool {
        self.status == BillingEventStatus::Failed
    }

    pub fn is_pending(&self) -> bool {
        self.status == BillingEventStatus::Pending
    }

    pub fn processing_time(&self) -> Option<chrono::Duration> {
        self.processed_at.map(|processed| processed - self.created_at)
    }
}

// Common event types
impl DeploymentBillingEvent {
    pub const EVENT_STRIPE_ACCOUNT_CREATED: &'static str = "stripe.account.created";
    pub const EVENT_STRIPE_ACCOUNT_UPDATED: &'static str = "stripe.account.updated";
    pub const EVENT_SUBSCRIPTION_CREATED: &'static str = "subscription.created";
    pub const EVENT_SUBSCRIPTION_UPDATED: &'static str = "subscription.updated";
    pub const EVENT_SUBSCRIPTION_CANCELED: &'static str = "subscription.canceled";
    pub const EVENT_INVOICE_CREATED: &'static str = "invoice.created";
    pub const EVENT_INVOICE_PAID: &'static str = "invoice.paid";
    pub const EVENT_INVOICE_PAYMENT_FAILED: &'static str = "invoice.payment_failed";
    pub const EVENT_PAYMENT_METHOD_ATTACHED: &'static str = "payment_method.attached";
    pub const EVENT_PAYMENT_METHOD_DETACHED: &'static str = "payment_method.detached";
    pub const EVENT_USAGE_RECORDED: &'static str = "usage.recorded";
}