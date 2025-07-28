use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Incomplete,
    IncompleteExpired,
    Trialing,
    Active,
    PastDue,
    Canceled,
    Unpaid,
    Paused,
}

impl std::fmt::Display for SubscriptionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubscriptionStatus::Incomplete => write!(f, "incomplete"),
            SubscriptionStatus::IncompleteExpired => write!(f, "incomplete_expired"),
            SubscriptionStatus::Trialing => write!(f, "trialing"),
            SubscriptionStatus::Active => write!(f, "active"),
            SubscriptionStatus::PastDue => write!(f, "past_due"),
            SubscriptionStatus::Canceled => write!(f, "canceled"),
            SubscriptionStatus::Unpaid => write!(f, "unpaid"),
            SubscriptionStatus::Paused => write!(f, "paused"),
        }
    }
}

impl std::str::FromStr for SubscriptionStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "incomplete" => Ok(SubscriptionStatus::Incomplete),
            "incomplete_expired" => Ok(SubscriptionStatus::IncompleteExpired),
            "trialing" => Ok(SubscriptionStatus::Trialing),
            "active" => Ok(SubscriptionStatus::Active),
            "past_due" => Ok(SubscriptionStatus::PastDue),
            "canceled" => Ok(SubscriptionStatus::Canceled),
            "unpaid" => Ok(SubscriptionStatus::Unpaid),
            "paused" => Ok(SubscriptionStatus::Paused),
            _ => Err(format!("Invalid subscription status: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for SubscriptionStatus {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for SubscriptionStatus {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for SubscriptionStatus {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CollectionMethod {
    ChargeAutomatically,
    SendInvoice,
}

impl std::fmt::Display for CollectionMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollectionMethod::ChargeAutomatically => write!(f, "charge_automatically"),
            CollectionMethod::SendInvoice => write!(f, "send_invoice"),
        }
    }
}

impl std::str::FromStr for CollectionMethod {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "charge_automatically" => Ok(CollectionMethod::ChargeAutomatically),
            "send_invoice" => Ok(CollectionMethod::SendInvoice),
            _ => Err(format!("Invalid collection method: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for CollectionMethod {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for CollectionMethod {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for CollectionMethod {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct DeploymentSubscription {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub user_id: Option<i64>,
    pub stripe_subscription_id: String,
    pub stripe_customer_id: String,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub billing_plan_id: Option<i64>,
    pub status: SubscriptionStatus,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub trial_start: Option<DateTime<Utc>>,
    pub trial_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub collection_method: CollectionMethod,
    pub customer_email: Option<String>,
    pub customer_name: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentSubscriptionWithPlan {
    #[serde(flatten)]
    pub subscription: DeploymentSubscription,
    pub billing_plan: Option<super::DeploymentBillingPlan>,
}

impl DeploymentSubscription {
    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            SubscriptionStatus::Active | SubscriptionStatus::Trialing
        )
    }

    pub fn is_in_trial(&self) -> bool {
        self.status == SubscriptionStatus::Trialing
    }

    pub fn is_canceled(&self) -> bool {
        matches!(
            self.status,
            SubscriptionStatus::Canceled | SubscriptionStatus::IncompleteExpired
        )
    }

    pub fn is_past_due(&self) -> bool {
        matches!(
            self.status,
            SubscriptionStatus::PastDue | SubscriptionStatus::Unpaid
        )
    }

    pub fn days_until_period_end(&self) -> i64 {
        let now = Utc::now();
        (self.current_period_end - now).num_days()
    }

    pub fn trial_days_remaining(&self) -> Option<i64> {
        self.trial_end.map(|end| {
            let now = Utc::now();
            (end - now).num_days().max(0)
        })
    }
}