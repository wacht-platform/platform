use axum::http::StatusCode;
use chrono::{Duration, Utc};
use models::billing::{BillingAccountWithSubscription, Subscription};
use queries::billing::UsageSnapshot;
use serde::{Deserialize, Serialize};
use wacht::middleware::AuthContext;

#[derive(Debug, Deserialize)]
pub struct CreateCheckoutRequest {
    pub plan_name: String,
    pub legal_name: String,
    pub billing_email: String,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub return_url: String,
}

#[derive(Debug, Serialize)]
pub struct CheckoutResponse {
    pub requires_checkout: bool,
    pub checkout_id: Option<String>,
    pub checkout_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PortalResponse {
    pub portal_url: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBillingAccountRequest {
    pub legal_name: Option<String>,
    pub billing_email: Option<String>,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChangePlanRequest {
    pub plan_name: String,
    pub proration_mode: Option<String>,
    pub return_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub snapshots: Vec<UsageSnapshot>,
    pub billing_period: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatePulseCheckoutRequest {
    pub pulse_amount: i64,
    pub return_url: String,
}

pub(super) fn owner_id_from_auth(auth: &AuthContext) -> String {
    if let Some(org_id) = &auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    }
}

pub(super) fn owner_type_from_owner_id(owner_id: &str) -> &'static str {
    if owner_id.starts_with("org_") {
        "organization"
    } else {
        "user"
    }
}

pub(super) fn enforce_checkout_cooldown(
    account: &BillingAccountWithSubscription,
) -> Result<(), (StatusCode, String)> {
    if let Some(last_created_at) = account.billing_account.last_checkout_session_created_at {
        let next_allowed_at = last_created_at + Duration::minutes(2);
        if next_allowed_at > Utc::now() {
            let wait_seconds = (next_allowed_at - Utc::now()).num_seconds().max(1);
            return Err((
                StatusCode::TOO_MANY_REQUESTS,
                format!(
                    "Checkout already generated recently. Please retry in {} seconds.",
                    wait_seconds
                ),
            ));
        }
    }

    Ok(())
}

pub(super) fn is_local_starter_subscription(subscription: &Subscription) -> bool {
    subscription
        .provider_subscription_id
        .starts_with("local_starter_")
}

pub(super) fn starter_activation_response() -> CheckoutResponse {
    CheckoutResponse {
        requires_checkout: false,
        checkout_id: None,
        checkout_url: None,
    }
}
