use axum::{extract::State, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::application::response::{ApiResult, PaginatedResponse};

use commands::{
    Command,
    billing::{
        CreateBillingAccountCommand, SetProviderCustomerIdCommand, UpdateBillingAccountCommand,
    },
};
use common::dodo::{
    ChangePlanParams, CheckoutCustomer, CreateCheckoutParams, CreateCustomerParams, DodoClient,
    ProductCartItem,
};
use common::state::AppState;
use models::billing::BillingAccountWithSubscription;
use queries::{
    Query as QueryTrait,
    billing::{
        GetBillingAccountQuery, GetBillingAccountUsageQuery, GetDodoProductQuery, UsageSnapshot,
    },
};
use wacht::middleware::RequireAuth;


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
    pub checkout_id: String,
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
) -> ApiResult<Option<BillingAccountWithSubscription>> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await?;

    Ok(account.into())
}

pub async fn create_checkout(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateCheckoutRequest>,
) -> ApiResult<CheckoutResponse> {
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
        .await?;

    if let Some(account) = existing.clone() {
        if account.subscription.is_some() {
            return Err((StatusCode::CONFLICT, "Subscription already exists").into());
        }
    }

    let dodo = DodoClient::new().map_err(|e| {
        error!("Failed to create Dodo client: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Payment gateway initialization failed")
    })?;

    let provider_customer_id = if let Some(ref account) = existing {
        if let Some(ref cid) = account.billing_account.provider_customer_id {
            cid.clone()
        } else {
            let customer = dodo
                .create_customer(CreateCustomerParams {
                    email: req.billing_email.clone(),
                    name: Some(req.legal_name.clone()),
                    metadata: Some(serde_json::json!({ "owner_id": owner_id })),
                })
                .await
                .map_err(|e| {
                    error!("Failed to create Dodo customer: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create customer")
                })?;

            SetProviderCustomerIdCommand {
                owner_id: owner_id.clone(),
                provider_customer_id: customer.customer_id.clone(),
            }
            .execute(&state)
            .await?;

            customer.customer_id
        }
    } else {
        CreateBillingAccountCommand {
            owner_id: owner_id.clone(),
            owner_type: owner_type.to_string(),
            legal_name: req.legal_name.clone(),
            billing_email: req.billing_email.clone(),
            billing_phone: req.billing_phone.clone(),
            tax_id: req.tax_id.clone(),
            address_line1: None,
            address_line2: None,
            city: None,
            state: None,
            postal_code: None,
            country: None,
        }
        .execute(&state)
        .await?;

        let customer = dodo
            .create_customer(CreateCustomerParams {
                email: req.billing_email.clone(),
                name: Some(req.legal_name.clone()),
                metadata: Some(serde_json::json!({ "owner_id": owner_id })),
            })
            .await
            .map_err(|e| {
                error!("Failed to create Dodo customer: {}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create customer")
            })?;

        SetProviderCustomerIdCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer.customer_id.clone(),
        }
        .execute(&state)
        .await?;

        customer.customer_id
    };

    let product = GetDodoProductQuery::new(&req.plan_name)
        .execute(&state)
        .await?
        .ok_or_else(|| {
            error!("Product not found for plan: {}", req.plan_name);
            (StatusCode::NOT_FOUND, "Plan not found")
        })?;

    let params = CreateCheckoutParams {
        product_cart: vec![ProductCartItem {
            product_id: product.product_id,
            quantity: 1,
            amount: None,
        }],
        return_url: req.return_url,
        customer: Some(CheckoutCustomer {
            customer_id: Some(provider_customer_id),
            email: Some(req.billing_email),
            name: Some(req.legal_name),
        }),
        metadata: Some(serde_json::json!({
            "owner_id": owner_id,
            "owner_type": owner_type,
        })),
        discount_code: None,
    };

    let checkout = dodo.create_checkout_session(params).await.map_err(|e| {
        error!("Failed to create checkout session: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Checkout initialization failed")
    })?;

    Ok(CheckoutResponse {
        checkout_id: checkout.checkout_id,
        checkout_url: checkout.checkout_url,
    }
    .into())
}

pub async fn update_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<UpdateBillingAccountRequest>,
) -> ApiResult<()> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let existing = GetBillingAccountQuery::new(owner_id.clone())
        .execute(&state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

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

    command
        .execute(&state)
        .await?;

    Ok(().into())
}

pub async fn get_portal_url(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PortalResponse> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

    let provider_customer_id = account
        .billing_account
        .provider_customer_id
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Payment provider customer not found"))?;

    let dodo = DodoClient::new().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let portal = dodo
        .create_portal_session(&provider_customer_id)
        .await
        .map_err(|e| {
            error!("Failed to create portal session: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create portal session")
        })?;

    Ok(PortalResponse {
        portal_url: portal.url,
    }
    .into())
}



pub async fn list_invoices(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<serde_json::Value> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await?;

    if let Some(account) = account {
        if let Some(provider_customer_id) = account.billing_account.provider_customer_id {
            let dodo = DodoClient::new().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

            let payments = dodo
                .list_payments(Some(&provider_customer_id))
                .await
                .map_err(|e| {
                    error!("Failed to list payments: {}", e);
                    (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list payments")
                })?;

            return Ok(serde_json::json!({ "items": payments.items }).into());
        }
    }

    Ok(serde_json::json!({ "items": [] }).into())
}


#[derive(Debug, Deserialize)]
pub struct ChangePlanRequest {
    pub plan_name: String,
    pub proration_mode: Option<String>,
}

pub async fn change_plan(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<ChangePlanRequest>,
) -> ApiResult<()> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

    let subscription = account.subscription.ok_or_else(|| (StatusCode::NOT_FOUND, "Subscription not found"))?;

    let product = GetDodoProductQuery::new(&req.plan_name)
        .execute(&state)
        .await?
        .ok_or_else(|| {
            error!("Product not found for plan: {}", req.plan_name);
            (StatusCode::NOT_FOUND, "Plan not found")
        })?;

    let dodo = DodoClient::new().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    dodo.change_plan(
        &subscription.provider_subscription_id,
        ChangePlanParams {
            product_id: product.product_id,
            proration_mode: req.proration_mode,
        },
    )
    .await
    .map_err(|e| {
        error!("Failed to change plan: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to change plan")
    })?;

    Ok(().into())
}


#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub snapshots: Vec<UsageSnapshot>,
    pub billing_period: String,
}

pub async fn get_current_usage(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<UsageResponse> {
    // 1. Determine owner_id from auth context
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    // 2. Fetch Billing Account & Subscription
    let account_with_sub = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "Billing account not found",
            )
        })?;

    let subscription = account_with_sub.subscription.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "No active subscription found for this billing account",
        )
    })?;

    // 3. Get Billing Period (Start Date)
    let billing_period_timestamp = subscription
        .previous_billing_date
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Subscription missing previous_billing_date",
            )
        })?;

    // 4. Fetch Usage for the Billing Account
    let snapshots = GetBillingAccountUsageQuery::new(
        account_with_sub.billing_account.id,
        billing_period_timestamp,
    )
    .execute(&state)
    .await?;

    Ok(UsageResponse {
        snapshots,
        billing_period: billing_period_timestamp.to_rfc3339(),
    }
    .into())
}
#[derive(Debug, Deserialize)]
pub struct CreatePulseCheckoutRequest {
    pub pulse_amount: i64, // e.g., 1000 for $10 worth of Pulse
    pub return_url: String,
}

pub async fn create_pulse_checkout(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreatePulseCheckoutRequest>,
) -> ApiResult<CheckoutResponse> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let existing = GetBillingAccountQuery::new(owner_id.clone())
        .execute(&state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

    let provider_customer_id = existing
        .billing_account
        .provider_customer_id
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Payment provider customer not found"))?;

    let product = GetDodoProductQuery::new("pulse_credits")
        .execute(&state)
        .await?
        .ok_or_else(|| {
            error!("Product 'pulse_credits' not found");
            (StatusCode::INTERNAL_SERVER_ERROR, "Pulse product configuration missing")
        })?;

    let total_charge = ((req.pulse_amount + 50) as f64 / 0.96).ceil() as i64;

    let dodo = DodoClient::new().map_err(|e| {
        error!("Failed to create Dodo client: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Payment gateway initialization failed")
    })?;

    let params = CreateCheckoutParams {
        product_cart: vec![ProductCartItem {
            product_id: product.product_id,
            quantity: 1,
            amount: Some(total_charge),
        }],
        return_url: req.return_url,
        customer: Some(CheckoutCustomer {
            customer_id: Some(provider_customer_id),
            email: Some(existing.billing_account.billing_email),
            name: Some(existing.billing_account.legal_name),
        }),
        metadata: Some(serde_json::json!({
            "type": "pulse_purchase",
            "owner_id": owner_id,
        })),
        discount_code: None,
    };

    let checkout = dodo.create_checkout_session(params).await.map_err(|e| {
        error!("Failed to create pulse checkout session: {}", e);
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create checkout session")
    })?;

    Ok(CheckoutResponse {
        checkout_id: checkout.checkout_id,
        checkout_url: checkout.checkout_url,
    }
    .into())
}
pub async fn list_pulse_transactions(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PaginatedResponse<models::pulse_transaction::PulseTransaction>> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let account = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

    let transactions = queries::billing::ListPulseTransactionsQuery::new(account.billing_account.id)
        .execute(&state)
        .await?;

    Ok(PaginatedResponse::from(transactions).into())
}
