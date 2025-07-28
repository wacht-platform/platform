use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// Stripe Connect requests
#[derive(Debug, Deserialize)]
pub struct InitiateStripeConnectRequest {
    pub account_type: String, // "express" | "standard"
    pub country: Option<String>,
    #[serde(default)]
    pub refresh_url: Option<String>,
    #[serde(default)]
    pub return_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct StripeConnectResponse {
    pub onboarding_url: String,
    pub account_id: String,
}

#[derive(Debug, Serialize)]
pub struct StripeAccountStatusResponse {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub stripe_account_id: String,
    pub account_type: String,
    pub charges_enabled: bool,
    pub details_submitted: bool,
    pub setup_completed_at: Option<DateTime<Utc>>,
    pub dashboard_url: Option<String>,
    pub country: Option<String>,
    pub default_currency: Option<String>,
    pub is_setup_complete: bool,
}

// Billing plan requests
#[derive(Debug, Deserialize)]
pub struct CreateBillingPlanRequest {
    pub name: String,
    pub description: Option<String>,
    pub amount_cents: i64,
    pub currency: Option<String>,
    pub billing_interval: String, // "month" | "year" | "week" | "day"
    pub trial_period_days: Option<i32>,
    pub usage_type: Option<String>, // "licensed" | "metered"
    pub features: Option<Value>,
    pub display_order: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBillingPlanRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub trial_period_days: Option<i32>,
    pub features: Option<Value>,
    pub is_active: Option<bool>,
    pub display_order: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct BillingPlanResponse {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub description: Option<String>,
    pub stripe_price_id: String,
    pub billing_interval: String,
    pub amount_cents: i64,
    pub amount: f64, // amount in currency units
    pub currency: String,
    pub trial_period_days: Option<i32>,
    pub usage_type: Option<String>,
    pub features: Value,
    pub is_active: bool,
    pub display_order: i32,
    pub is_trial_available: bool,
    pub is_metered: bool,
}

// Subscription requests
#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionRequest {
    pub billing_plan_id: i64,
    pub customer_email: String,
    pub customer_name: Option<String>,
    pub payment_method_id: Option<String>,
    pub trial_period_days: Option<i32>,
    pub collection_method: Option<String>, // "charge_automatically" | "send_invoice"
    pub metadata: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateSubscriptionRequest {
    pub billing_plan_id: Option<i64>,
    pub cancel_at_period_end: Option<bool>,
    pub collection_method: Option<String>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionResponse {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string_option")]
    pub user_id: Option<i64>,
    pub stripe_subscription_id: String,
    pub stripe_customer_id: String,
    pub status: String,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub trial_start: Option<DateTime<Utc>>,
    pub trial_end: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub canceled_at: Option<DateTime<Utc>>,
    pub ended_at: Option<DateTime<Utc>>,
    pub collection_method: String,
    pub customer_email: Option<String>,
    pub customer_name: Option<String>,
    pub metadata: Value,
    pub billing_plan: Option<BillingPlanResponse>,
    // Computed fields
    pub is_active: bool,
    pub is_in_trial: bool,
    pub is_canceled: bool,
    pub is_past_due: bool,
    pub days_until_period_end: i64,
    pub trial_days_remaining: Option<i64>,
}

// Invoice responses
#[derive(Debug, Serialize)]
pub struct InvoiceResponse {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub stripe_invoice_id: String,
    pub stripe_customer_id: String,
    pub amount_due_cents: i64,
    pub amount_due: f64, // amount in currency units
    pub amount_paid_cents: i64,
    pub amount_paid: f64, // amount in currency units
    pub amount_remaining_cents: i64,
    pub amount_remaining: f64, // amount in currency units
    pub currency: String,
    pub status: String,
    pub invoice_pdf_url: Option<String>,
    pub hosted_invoice_url: Option<String>,
    pub invoice_number: Option<String>,
    pub due_date: Option<DateTime<Utc>>,
    pub paid_at: Option<DateTime<Utc>>,
    pub period_start: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub attempt_count: i32,
    pub next_payment_attempt: Option<DateTime<Utc>>,
    pub metadata: Value,
    // Computed fields
    pub is_paid: bool,
    pub is_overdue: bool,
    pub days_until_due: Option<i64>,
    pub is_partially_paid: bool,
}

// Payment method requests
#[derive(Debug, Deserialize)]
pub struct CreatePaymentMethodRequest {
    pub stripe_payment_method_id: String,
    pub stripe_customer_id: String,
    pub set_as_default: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PaymentMethodResponse {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub stripe_payment_method_id: String,
    pub stripe_customer_id: String,
    pub payment_method_type: String,
    pub is_default: bool,
    pub card_brand: Option<String>,
    pub card_last4: Option<String>,
    pub card_exp_month: Option<i32>,
    pub card_exp_year: Option<i32>,
    pub bank_name: Option<String>,
    pub bank_last4: Option<String>,
    pub metadata: Value,
    // Computed fields
    pub display_name: String,
    pub is_card: bool,
    pub is_bank_account: bool,
    pub is_expired: bool,
    pub expires_soon: bool,
}

// Usage tracking requests
#[derive(Debug, Deserialize)]
pub struct RecordUsageRequest {
    pub metric_name: String,
    pub quantity: i64,
    pub timestamp: Option<DateTime<Utc>>,
    pub metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct UsageRecordResponse {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub metric_name: String,
    pub quantity: i64,
    pub timestamp: DateTime<Utc>,
    pub stripe_usage_record_id: Option<String>,
    pub billing_period_start: DateTime<Utc>,
    pub billing_period_end: DateTime<Utc>,
    pub metadata: Value,
    // Computed fields
    pub is_in_current_billing_period: bool,
    pub is_synced_to_stripe: bool,
}

#[derive(Debug, Serialize)]
pub struct UsageMetricSummaryResponse {
    pub metric_name: String,
    pub total_quantity: i64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub record_count: i64,
    // Computed fields
    pub average_per_day: f64,
    pub average_per_record: f64,
}

#[derive(Debug, Serialize)]
pub struct UsageSummaryResponse {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub metrics: Vec<UsageMetricSummaryResponse>,
    pub total_records: i64,
}

// Billing dashboard response
#[derive(Debug, Serialize)]
pub struct BillingDashboardResponse {
    pub stripe_account: Option<StripeAccountStatusResponse>,
    pub active_subscriptions: Vec<SubscriptionResponse>,
    pub recent_invoices: Vec<InvoiceResponse>,
    pub billing_plans: Vec<BillingPlanResponse>,
    pub payment_methods: Vec<PaymentMethodResponse>,
    pub usage_summary: Option<UsageSummaryResponse>,
}

// Query parameters
#[derive(Debug, Deserialize)]
pub struct BillingListQuery {
    pub limit: Option<i32>,
    pub offset: Option<i32>,
    pub status: Option<String>,
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct UsageQuery {
    pub metric_name: Option<String>,
    pub from_date: Option<DateTime<Utc>>,
    pub to_date: Option<DateTime<Utc>>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}