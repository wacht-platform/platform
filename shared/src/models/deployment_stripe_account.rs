use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StripeAccountType {
    Standard,
    Express,
    Custom,
}

impl std::fmt::Display for StripeAccountType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StripeAccountType::Standard => write!(f, "standard"),
            StripeAccountType::Express => write!(f, "express"),
            StripeAccountType::Custom => write!(f, "custom"),
        }
    }
}

impl std::str::FromStr for StripeAccountType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "standard" => Ok(StripeAccountType::Standard),
            "express" => Ok(StripeAccountType::Express),
            "custom" => Ok(StripeAccountType::Custom),
            _ => Err(format!("Invalid stripe account type: {}", s)),
        }
    }
}

impl sqlx::Type<sqlx::Postgres> for StripeAccountType {
    fn type_info() -> sqlx::postgres::PgTypeInfo {
        sqlx::postgres::PgTypeInfo::with_name("TEXT")
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Postgres> for StripeAccountType {
    fn decode(value: sqlx::postgres::PgValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <&str as sqlx::Decode<sqlx::Postgres>>::decode(value)?;
        value.parse().map_err(|e: String| e.into())
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Postgres> for StripeAccountType {
    fn encode_by_ref(&self, buf: &mut sqlx::postgres::PgArgumentBuffer) -> Result<sqlx::encode::IsNull, Box<dyn std::error::Error + Send + Sync>> {
        <String as sqlx::Encode<sqlx::Postgres>>::encode(self.to_string(), buf)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentStripeAccount {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub stripe_account_id: String,
    pub stripe_user_id: Option<String>,
    pub access_token_encrypted: Option<String>,
    pub refresh_token_encrypted: Option<String>,
    pub account_type: StripeAccountType,
    pub charges_enabled: bool,
    pub details_submitted: bool,
    pub setup_completed_at: Option<DateTime<Utc>>,
    pub onboarding_url: Option<String>,
    pub dashboard_url: Option<String>,
    pub country: Option<String>,
    pub default_currency: Option<String>,
    pub metadata: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeploymentStripeAccountDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub stripe_account_id: String,
    pub account_type: StripeAccountType,
    pub charges_enabled: bool,
    pub details_submitted: bool,
    pub setup_completed_at: Option<DateTime<Utc>>,
    pub dashboard_url: Option<String>,
    pub country: Option<String>,
    pub default_currency: Option<String>,
    pub is_setup_complete: bool,
}

impl From<DeploymentStripeAccount> for DeploymentStripeAccountDetails {
    fn from(account: DeploymentStripeAccount) -> Self {
        Self {
            id: account.id,
            created_at: account.created_at,
            updated_at: account.updated_at,
            deployment_id: account.deployment_id,
            stripe_account_id: account.stripe_account_id,
            account_type: account.account_type,
            charges_enabled: account.charges_enabled,
            details_submitted: account.details_submitted,
            setup_completed_at: account.setup_completed_at,
            dashboard_url: account.dashboard_url,
            country: account.country,
            default_currency: account.default_currency,
            is_setup_complete: account.charges_enabled && account.details_submitted,
        }
    }
}