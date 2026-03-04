use axum::{extract::State, http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::application::response::{ApiResult, PaginatedResponse};

use chrono::{Duration, Utc};
use commands::{
    billing::{
        CreateBillingAccountCommand, MarkCheckoutSessionCreatedCommand,
        SetProviderCustomerIdCommand, UpdateBillingAccountCommand,
        UpdateBillingAccountStatusCommand, UpsertSubscriptionCommand,
    },
    Command,
};
use common::dodo::{
    ChangePlanParams, CheckoutCustomer, CreateCheckoutParams, CreateCustomerParams, DodoClient,
    ProductCartItem, UpdateCustomerParams,
};
use common::state::AppState;
use models::billing::{BillingAccountWithSubscription, Subscription};
use queries::{
    billing::{
        GetBillingAccountQuery, GetBillingAccountUsageQuery, GetDodoProductQuery, UsageSnapshot,
    },
    Query as QueryTrait,
};
use wacht::middleware::RequireAuth;

const STARTER_PRODUCT_ID_FALLBACK: &str = "pdt_6eSgfwefWhNkDH53uKxf8";

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

fn enforce_checkout_cooldown(
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

fn is_local_starter_subscription(subscription: &Subscription) -> bool {
    subscription
        .provider_subscription_id
        .starts_with("local_starter_")
}

fn starter_activation_response() -> CheckoutResponse {
    CheckoutResponse {
        requires_checkout: false,
        checkout_id: None,
        checkout_url: None,
    }
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

    let is_starter_plan = req.plan_name.eq_ignore_ascii_case("starter");

    let existing = GetBillingAccountQuery::new(owner_id.clone())
        .execute(&state)
        .await?;

    if let Some(account) = existing.clone() {
        if let Some(ref subscription) = account.subscription {
            if subscription.status == "active" {
                return Err((StatusCode::CONFLICT, "Subscription already exists").into());
            }
        }
        if !is_starter_plan {
            if let Err(err) = enforce_checkout_cooldown(&account) {
                return Err(err.into());
            }
        }
    }

    let dodo = DodoClient::new().map_err(|e| {
        error!("Failed to create Dodo client: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Payment gateway initialization failed",
        )
    })?;

    let provider_customer_id = if let Some(ref account) = existing {
        UpdateBillingAccountCommand {
            id: account.billing_account.id,
            legal_name: Some(req.legal_name.clone()),
            billing_email: Some(req.billing_email.clone()),
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

        if let Some(ref cid) = account.billing_account.provider_customer_id {
            let _ = dodo
                .update_customer(
                    cid,
                    UpdateCustomerParams {
                        email: Some(req.billing_email.clone()),
                        name: Some(req.legal_name.clone()),
                        metadata: None,
                    },
                )
                .await;

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
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Failed to create customer",
                    )
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
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Failed to create customer",
                )
            })?;

        SetProviderCustomerIdCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer.customer_id.clone(),
        }
        .execute(&state)
        .await?;

        customer.customer_id
    };

    if is_starter_plan {
        let starter_product_id = GetDodoProductQuery::new("starter")
            .execute(&state)
            .await?
            .map(|p| p.product_id)
            .unwrap_or_else(|| STARTER_PRODUCT_ID_FALLBACK.to_string());

        UpsertSubscriptionCommand {
            owner_id: owner_id.clone(),
            provider_customer_id,
            provider_subscription_id: format!("local_starter_{}", owner_id),
            product_id: Some(starter_product_id),
            status: "active".to_string(),
            previous_billing_date: Some(Utc::now()),
        }
        .execute(&state)
        .await?;

        UpdateBillingAccountStatusCommand {
            owner_id,
            status: "active".to_string(),
        }
        .execute(&state)
        .await?;

        return Ok(starter_activation_response().into());
    }

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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Checkout initialization failed",
        )
    })?;

    MarkCheckoutSessionCreatedCommand {
        owner_id: owner_id.clone(),
        checkout_session_id: checkout.checkout_id.clone(),
    }
    .execute(&state)
    .await?;

    Ok(CheckoutResponse {
        requires_checkout: true,
        checkout_id: Some(checkout.checkout_id),
        checkout_url: Some(checkout.checkout_url),
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

    if let Err(err) = enforce_checkout_cooldown(&existing) {
        return Err(err.into());
    }

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

    command.execute(&state).await?;

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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create portal session",
            )
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
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

    let invoices = queries::billing::ListBillingInvoicesQuery::new(account.billing_account.id)
        .execute(&state)
        .await?;

    Ok(serde_json::json!({ "items": invoices }).into())
}

#[derive(Debug, Deserialize)]
pub struct ChangePlanRequest {
    pub plan_name: String,
    pub proration_mode: Option<String>,
    pub return_url: Option<String>,
}

pub async fn change_plan(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<ChangePlanRequest>,
) -> ApiResult<CheckoutResponse> {
    let owner_id = if let Some(org_id) = auth.organization_id {
        format!("org_{}", org_id)
    } else {
        format!("user_{}", auth.user_id)
    };

    let account = GetBillingAccountQuery::new(owner_id.clone())
        .execute(&state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

    let subscription = account
        .subscription
        .as_ref()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Subscription not found"))?;

    let product = GetDodoProductQuery::new(&req.plan_name)
        .execute(&state)
        .await?
        .ok_or_else(|| {
            error!("Product not found for plan: {}", req.plan_name);
            (StatusCode::NOT_FOUND, "Plan not found")
        })?;

    let dodo = DodoClient::new().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if is_local_starter_subscription(&subscription) {
        if let Err(err) = enforce_checkout_cooldown(&account) {
            return Err(err.into());
        }

        let return_url = req.return_url.ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                "return_url is required for starter upgrades",
            )
        })?;

        let provider_customer_id = account
            .billing_account
            .provider_customer_id
            .clone()
            .filter(|v| !v.is_empty())
            .unwrap_or(subscription.provider_customer_id.clone());

        let params = CreateCheckoutParams {
            product_cart: vec![ProductCartItem {
                product_id: product.product_id,
                quantity: 1,
                amount: None,
            }],
            return_url,
            customer: Some(CheckoutCustomer {
                customer_id: Some(provider_customer_id),
                email: Some(account.billing_account.billing_email),
                name: Some(account.billing_account.legal_name),
            }),
            metadata: Some(serde_json::json!({
                "owner_id": owner_id.clone(),
                "owner_type": account.billing_account.owner_type,
            })),
            discount_code: None,
        };

        let checkout = dodo.create_checkout_session(params).await.map_err(|e| {
            error!("Failed to create checkout session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Checkout initialization failed",
            )
        })?;

        MarkCheckoutSessionCreatedCommand {
            owner_id,
            checkout_session_id: checkout.checkout_id.clone(),
        }
        .execute(&state)
        .await?;

        return Ok(CheckoutResponse {
            requires_checkout: true,
            checkout_id: Some(checkout.checkout_id),
            checkout_url: Some(checkout.checkout_url),
        }
        .into());
    }

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

    Ok(CheckoutResponse {
        requires_checkout: false,
        checkout_id: None,
        checkout_url: None,
    }
    .into())
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
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Billing account not found"))?;

    let subscription = account_with_sub.subscription.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "No active subscription found for this billing account",
        )
    })?;

    // 3. Get Billing Period (Start Date)
    let billing_period_timestamp = subscription.previous_billing_date.ok_or_else(|| {
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
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Pulse product configuration missing",
            )
        })?;

    let total_charge = ((req.pulse_amount + 50) as f64 / 0.96).ceil() as i64;

    let dodo = DodoClient::new().map_err(|e| {
        error!("Failed to create Dodo client: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Payment gateway initialization failed",
        )
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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to create checkout session",
        )
    })?;

    MarkCheckoutSessionCreatedCommand {
        owner_id,
        checkout_session_id: checkout.checkout_id.clone(),
    }
    .execute(&state)
    .await?;

    Ok(CheckoutResponse {
        requires_checkout: true,
        checkout_id: Some(checkout.checkout_id),
        checkout_url: Some(checkout.checkout_url),
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

    let transactions =
        queries::billing::ListPulseTransactionsQuery::new(account.billing_account.id)
            .execute(&state)
            .await?;

    Ok(PaginatedResponse::from(transactions).into())
}
