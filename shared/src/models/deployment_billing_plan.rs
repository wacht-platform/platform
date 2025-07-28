use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BillingInterval {
    Month,
    Year,
    Week,
    Day,
}

impl std::fmt::Display for BillingInterval {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BillingInterval::Month => write!(f, "month"),
            BillingInterval::Year => write!(f, "year"),
            BillingInterval::Week => write!(f, "week"),
            BillingInterval::Day => write!(f, "day"),
        }
    }
}

impl std::str::FromStr for BillingInterval {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "month" => Ok(BillingInterval::Month),
            "year" => Ok(BillingInterval::Year),
            "week" => Ok(BillingInterval::Week),
            "day" => Ok(BillingInterval::Day),
            _ => Err(format!("Invalid billing interval: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for BillingInterval {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for BillingInterval {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for BillingInterval {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BillingUsageType {
    Licensed,
    Metered,
}

impl std::fmt::Display for BillingUsageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BillingUsageType::Licensed => write!(f, "licensed"),
            BillingUsageType::Metered => write!(f, "metered"),
        }
    }
}

impl std::str::FromStr for BillingUsageType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "licensed" => Ok(BillingUsageType::Licensed),
            "metered" => Ok(BillingUsageType::Metered),
            _ => Err(format!("Invalid usage type: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for BillingUsageType {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for BillingUsageType {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for BillingUsageType {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct DeploymentBillingPlan {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub stripe_price_id: String,
    pub billing_interval: BillingInterval,
    pub amount_cents: i64,
    pub currency: String,
    pub trial_period_days: Option<i32>,
    pub usage_type: Option<BillingUsageType>,
    pub features: Value,
    pub is_active: bool,
    pub display_order: i32,
}

impl DeploymentBillingPlan {
    pub fn amount_in_currency_unit(&self) -> f64 {
        self.amount_cents as f64 / 100.0
    }

    pub fn is_trial_available(&self) -> bool {
        self.trial_period_days.map_or(false, |days| days > 0)
    }

    pub fn is_metered(&self) -> bool {
        matches!(self.usage_type, Some(BillingUsageType::Metered))
    }
}