use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};
use tracing::error;

use common::state::AppState;
use common::chargebee::{
    ChargebeeClient, CreateCheckoutParams, SubscriptionItem, CustomerInfo, BillingAddress,
    UpdateSubscriptionParams,
};
use models::billing::BillingAccountWithSubscription;
use queries::{
    Query as QueryTrait,
    billing::GetBillingAccountQuery,
};
use commands::{
    Command,
    billing::{CreateBillingAccountCommand, UpdateBillingAccountCommand, UpdateSubscriptionStatusCommand},
};
use wacht::middleware::RequireAuth;

#[derive(Debug, Deserialize)]
pub struct CreateCheckoutRequest {
    pub plan_id: String,
    pub legal_name: String,
    pub billing_email: String,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub address_line1: String,
    pub address_line2: Option<String>,
    pub city: String,
    pub state: Option<String>,
    pub postal_code: String,
    pub country: String,
}

#[derive(Debug, Serialize)]
pub struct CheckoutResponse {
    pub checkout_url: String,
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

pub async fn get_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> Result<Json<Option<BillingAccountWithSubscription>>, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get billing account: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok(Json(account))
}

pub async fn create_checkout(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateCheckoutRequest>,
) -> Result<Json<CheckoutResponse>, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let owner_type = if owner_id.starts_with("org_") {
        "organization"
    } else {
        "user"
    };
    
    let existing = GetBillingAccountQuery::new(owner_id.clone())
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get existing billing account: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    if let Some(account) = existing.clone() {
        if account.subscription.is_some() {
            return Err(StatusCode::CONFLICT);
        }
    }
    
    if existing.is_none() {
        let command = CreateBillingAccountCommand {
            owner_id: owner_id.clone(),
            owner_type: owner_type.to_string(),
            legal_name: req.legal_name.clone(),
            billing_email: req.billing_email.clone(),
            billing_phone: req.billing_phone.clone(),
            tax_id: req.tax_id.clone(),
            address_line1: req.address_line1.clone(),
            address_line2: req.address_line2.clone(),
            city: req.city.clone(),
            state: req.state.clone(),
            postal_code: req.postal_code.clone(),
            country: req.country.clone(),
        };
        
        command.execute(&state)
            .await
            .map_err(|e| {
                error!("Failed to create billing account: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
    }
    
    let chargebee = ChargebeeClient::new()
        .map_err(|e| {
            error!("Failed to create Chargebee client: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    let params = CreateCheckoutParams {
        subscription_items: vec![SubscriptionItem {
            item_price_id: req.plan_id,
            quantity: Some(1),
        }],
        customer: CustomerInfo {
            id: Some(owner_id.clone()),
            email: req.billing_email,
            first_name: Some(req.legal_name),
            last_name: None,
            company: None,
            phone: req.billing_phone.clone(),
            billing_address: Some(BillingAddress {
                line1: req.address_line1,
                line2: req.address_line2,
                city: req.city,
                state: req.state,
                zip: req.postal_code,
                country: req.country,
            }),
        },
    };
    
    let response = chargebee.create_checkout_session(params)
        .await
        .map_err(|e| {
            error!("Failed to create checkout session: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    let url = response["hosted_page"]["url"]
        .as_str()
        .unwrap_or("")
        .to_string();
    
    Ok(Json(CheckoutResponse {
        checkout_url: url,
    }))
}

pub async fn update_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<UpdateBillingAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let existing = GetBillingAccountQuery::new(owner_id.clone())
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let command = UpdateBillingAccountCommand {
        id: existing.billing_account.id,
        legal_name: req.legal_name,
        billing_email: req.billing_email,
        billing_phone: req.billing_phone,
        tax_id: req.tax_id,
        address_line1: req.address_line1,
        address_line2: req.address_line2,
        city: req.city,
        state: req.state,
        postal_code: req.postal_code,
        country: req.country,
    };
    
    command.execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(StatusCode::OK)
}

pub async fn get_portal_url(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> Result<Json<PortalResponse>, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let subscription = account.subscription
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let response = chargebee.create_portal_session(&subscription.chargebee_customer_id, None)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let url = response["portal_session"]["access_url"]
        .as_str()
        .unwrap_or("")
        .to_string();
    
    Ok(Json(PortalResponse {
        portal_url: url,
    }))
}

pub async fn cancel_subscription(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> Result<StatusCode, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let subscription = account.subscription
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    chargebee.cancel_subscription(&subscription.chargebee_subscription_id, true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    UpdateSubscriptionStatusCommand {
        subscription_id: subscription.id,
        status: "cancelled".to_string(),
    }
    .execute(&state)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct RecordUsageRequest {
    pub item_price_id: String,
    pub quantity: i64,
    pub usage_date: Option<i64>, // Unix timestamp
}

pub async fn record_usage(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<RecordUsageRequest>,
) -> Result<StatusCode, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get billing account: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let subscription = account.subscription
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let chargebee = ChargebeeClient::new()
        .map_err(|e| {
            error!("Failed to create Chargebee client: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    chargebee.record_usage(
        &subscription.chargebee_subscription_id,
        &req.item_price_id,
        req.quantity,
        req.usage_date,
    )
    .await
    .map_err(|e| {
        error!("Failed to record usage: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    Ok(StatusCode::OK)
}

// Invoice endpoints
pub async fn list_invoices(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    if let Some(account) = account {
        if let Some(subscription) = account.subscription {
            let chargebee = ChargebeeClient::new()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            
            let invoices = chargebee.list_invoices(Some(&subscription.chargebee_subscription_id), Some(20))
                .await
                .map_err(|e| {
                    error!("Failed to list invoices: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;
            
            return Ok(Json(invoices));
        }
    }
    
    // Return empty list if no subscription
    Ok(Json(serde_json::json!({ "list": [] })))
}

pub async fn get_invoice(
    State(_state): State<AppState>,
    RequireAuth(_auth): RequireAuth,
    axum::extract::Path(invoice_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let invoice = chargebee.get_invoice(&invoice_id)
        .await
        .map_err(|e| {
            error!("Failed to get invoice: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    
    Ok(Json(invoice))
}

// Plan change endpoint
#[derive(Debug, Deserialize)]
pub struct ChangePlanRequest {
    pub new_plan_id: String,
}

pub async fn change_plan(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<ChangePlanRequest>,
) -> Result<StatusCode, StatusCode> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };
    
    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let subscription = account.subscription
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Update subscription with new plan
    chargebee.update_subscription(
        &subscription.chargebee_subscription_id,
        UpdateSubscriptionParams {
            plan_id: Some(req.new_plan_id),
            plan_quantity: None,
            trial_end: None,
            invoice_immediately: Some(false), // Prorate at end of billing cycle
            invoice_immediately_min_amount: None,
        }
    )
    .await
    .map_err(|e| {
        error!("Failed to change plan: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    
    Ok(StatusCode::OK)
}