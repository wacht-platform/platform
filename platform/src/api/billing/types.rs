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
