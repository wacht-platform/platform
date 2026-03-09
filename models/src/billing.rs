use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct BillingAccount {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub owner_id: String,
    pub owner_type: String,
    pub provider_customer_id: Option<String>,
    pub legal_name: String,
    pub tax_id: Option<String>,
    pub billing_email: String,
    pub billing_phone: Option<String>,
    pub address_line1: String,
    pub address_line2: Option<String>,
    pub city: String,
    pub state: Option<String>,
    pub postal_code: String,
    pub country: String,
    pub status: String, // pending, active, cancelled, failed
    pub payment_method_status: Option<String>,
    pub currency: String,
    pub locale: String,
    pub pulse_balance_cents: i64,
    pub max_projects_per_account: i64,
    pub max_staging_deployments_per_project: i64,
    pub pulse_usage_disabled: bool,
    pub pulse_notified_below_five: bool,
    pub pulse_notified_below_zero: bool,
    pub pulse_notified_disabled: bool,
    pub last_checkout_session_id: Option<String>,
    pub checkout_flow_state: String,
    pub last_payment_succeeded_at: Option<DateTime<Utc>>,
    pub last_subscription_activated_at: Option<DateTime<Utc>>,
    pub last_billing_webhook_event: Option<String>,
    pub checkout_flow_error: Option<String>,
    pub last_checkout_session_created_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Subscription {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub billing_account_id: i64,
    pub provider_customer_id: String,
    pub provider_subscription_id: String,
    pub product_id: Option<String>,
    #[sqlx(default)]
    pub plan_name: Option<String>,
    pub status: String,
    pub previous_billing_date: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BillingAccountWithSubscription {
    #[serde(flatten)]
    pub billing_account: BillingAccount,
    pub subscription: Option<Subscription>,
}
