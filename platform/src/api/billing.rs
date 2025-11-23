use axum::{extract::State, http::StatusCode, response::Json};
use chrono::Datelike;
use serde::{Deserialize, Serialize};
use tracing::error;

use commands::{
    Command,
    billing::{CreateBillingAccountCommand, UpdateBillingAccountCommand, SetProviderCustomerIdCommand},
};
use common::dodo::{
    DodoClient, CreateCheckoutParams, ProductCartItem, CheckoutCustomer, ChangePlanParams,
    CreateCustomerParams,
};
use common::state::AppState;
use models::billing::BillingAccountWithSubscription;
use queries::{
    Query as QueryTrait,
    billing::{GetBillingAccountQuery, GetDeploymentUsageQuery, GetDodoProductQuery, GetAllDodoProductsQuery, DodoProduct, UsageSnapshot},
};
use wacht::middleware::RequireAuth;

use crate::middleware::RequireDeployment;

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

    let dodo = DodoClient::new().map_err(|e| {
        error!("Failed to create Dodo client: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
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
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            SetProviderCustomerIdCommand {
                owner_id: owner_id.clone(),
                provider_customer_id: customer.customer_id.clone(),
            }
            .execute(&state)
            .await
            .map_err(|e| {
                error!("Failed to save provider customer id: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

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
        .await
        .map_err(|e| {
            error!("Failed to create billing account: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        let customer = dodo
            .create_customer(CreateCustomerParams {
                email: req.billing_email.clone(),
                name: Some(req.legal_name.clone()),
                metadata: Some(serde_json::json!({ "owner_id": owner_id })),
            })
            .await
            .map_err(|e| {
                error!("Failed to create Dodo customer: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

        SetProviderCustomerIdCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer.customer_id.clone(),
        }
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to save provider customer id: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        customer.customer_id
    };

    let product = GetDodoProductQuery::new(&req.plan_name)
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get product: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            error!("Product not found for plan: {}", req.plan_name);
            StatusCode::NOT_FOUND
        })?;

    let params = CreateCheckoutParams {
        product_cart: vec![ProductCartItem {
            product_id: product.product_id,
            quantity: 1,
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

    let checkout = dodo
        .create_checkout_session(params)
        .await
        .map_err(|e| {
            error!("Failed to create checkout session: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(CheckoutResponse {
        checkout_id: checkout.checkout_id,
        checkout_url: checkout.checkout_url,
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

    command
        .execute(&state)
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

    let provider_customer_id = account
        .billing_account
        .provider_customer_id
        .ok_or(StatusCode::NOT_FOUND)?;

    let dodo = DodoClient::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let portal = dodo
        .create_portal_session(&provider_customer_id)
        .await
        .map_err(|e| {
            error!("Failed to create portal session: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(PortalResponse {
        portal_url: portal.url,
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

    let subscription = account.subscription.ok_or(StatusCode::NOT_FOUND)?;

    let dodo = DodoClient::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    dodo.cancel_subscription(&subscription.provider_subscription_id, "cancelled")
        .await
        .map_err(|e| {
            error!("Failed to cancel subscription: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(StatusCode::OK)
}

#[derive(Debug, Deserialize)]
pub struct RecordUsageRequest {
    pub event_name: String,
    pub quantity: i64,
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

    let account = GetBillingAccountQuery::new(owner_id.clone())
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get billing account: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or(StatusCode::NOT_FOUND)?;

    let provider_customer_id = account
        .billing_account
        .provider_customer_id
        .ok_or(StatusCode::NOT_FOUND)?;

    let dodo = DodoClient::new().map_err(|e| {
        error!("Failed to create Dodo client: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    let event_id = format!("{}_{}", owner_id, chrono::Utc::now().timestamp_millis());

    dodo.ingest_usage_events(
        &provider_customer_id,
        &req.event_name,
        req.quantity,
        &event_id,
        false,
    )
    .await
    .map_err(|e| {
        error!("Failed to record usage: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::OK)
}

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
        if let Some(provider_customer_id) = account.billing_account.provider_customer_id {
            let dodo = DodoClient::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            let payments = dodo
                .list_payments(Some(&provider_customer_id))
                .await
                .map_err(|e| {
                    error!("Failed to list payments: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR
                })?;

            return Ok(Json(serde_json::json!({ "items": payments.items })));
        }
    }

    Ok(Json(serde_json::json!({ "items": [] })))
}

pub async fn get_invoice(
    State(_state): State<AppState>,
    RequireAuth(_auth): RequireAuth,
    axum::extract::Path(payment_id): axum::extract::Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let dodo = DodoClient::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let invoice = dodo.get_payment_invoice(&payment_id).await.map_err(|e| {
        error!("Failed to get invoice: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(serde_json::to_value(invoice).unwrap_or_default()))
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

    let subscription = account.subscription.ok_or(StatusCode::NOT_FOUND)?;

    let product = GetDodoProductQuery::new(&req.plan_name)
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get product: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?
        .ok_or_else(|| {
            error!("Product not found for plan: {}", req.plan_name);
            StatusCode::NOT_FOUND
        })?;

    let dodo = DodoClient::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(StatusCode::OK)
}

pub async fn get_plans(
    State(state): State<AppState>,
) -> Result<Json<Vec<DodoProduct>>, StatusCode> {
    let products = GetAllDodoProductsQuery
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get plans: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(products))
}

#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub snapshots: Vec<UsageSnapshot>,
    pub billing_period: String,
}

pub async fn get_current_usage(
    State(state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> Result<Json<UsageResponse>, StatusCode> {
    let now = chrono::Utc::now();
    let billing_period = chrono::NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let snapshots = GetDeploymentUsageQuery::new(deployment_id, billing_period)
        .execute(&state)
        .await
        .map_err(|e| {
            error!("Failed to get deployment usage: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    Ok(Json(UsageResponse {
        snapshots,
        billing_period: format!("{}-{:02}", now.year(), now.month()),
    }))
}
