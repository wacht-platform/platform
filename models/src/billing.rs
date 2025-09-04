use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BillingAccount {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub owner_id: String,
    pub owner_type: String,
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
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Subscription {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub billing_account_id: i64,
    pub chargebee_customer_id: String,
    pub chargebee_subscription_id: String,
    pub status: String,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BillingAccountWithSubscription {
    #[serde(flatten)]
    pub billing_account: BillingAccount,
    pub subscription: Option<Subscription>,
}
