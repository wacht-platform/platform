use chrono::{Duration, Utc};
use commands::billing::{
    CreateBillingAccountCommand, MarkCheckoutSessionCreatedCommand, SetProviderCustomerIdCommand,
    UpdateBillingAccountCommand, UpdateBillingAccountStatusCommand, UpsertSubscriptionCommand,
};
use common::{
    dodo::{
        ChangePlanParams, CheckoutCustomer, CreateCheckoutParams, CreateCustomerParams, Customer,
        DodoClient, ListCustomersParams, ProductCartItem, UpdateCustomerParams,
    },
    error::AppError,
};
use models::billing::BillingAccountWithSubscription;
use queries::billing::{DodoProduct, GetBillingAccountQuery, GetDodoProductQuery};
use std::time::Duration as StdDuration;
use tracing::error;

use crate::application::AppState;

#[derive(Debug, Clone, Default)]
pub struct UpdateBillingAccountInput {
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

#[derive(Debug, Clone)]
pub struct CreateCheckoutInput {
    pub plan_name: String,
    pub legal_name: String,
    pub billing_email: String,
    pub billing_phone: Option<String>,
    pub tax_id: Option<String>,
    pub return_url: String,
}

#[derive(Debug, Clone)]
pub struct ChangePlanInput {
    pub plan_name: String,
    pub proration_mode: Option<String>,
    pub return_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreatePulseCheckoutInput {
    pub pulse_amount: i64,
    pub return_url: String,
}

#[derive(Debug, Clone)]
pub struct CheckoutOutcome {
    pub requires_checkout: bool,
    pub checkout_id: Option<String>,
    pub checkout_url: Option<String>,
}

const STARTER_PRODUCT_ID_FALLBACK: &str = "pdt_6eSgfwefWhNkDH53uKxf8";

fn enforce_checkout_cooldown(account: &BillingAccountWithSubscription) -> Result<(), AppError> {
    if let Some(last_created_at) = account.billing_account.last_checkout_session_created_at {
        let next_allowed_at = last_created_at + Duration::minutes(2);
        if next_allowed_at > Utc::now() {
            let wait_seconds = (next_allowed_at - Utc::now()).num_seconds().max(1);
            return Err(AppError::Validation(format!(
                "Checkout already generated recently. Please retry in {} seconds.",
                wait_seconds
            )));
        }
    }

    Ok(())
}

fn owner_type_from_owner_id(owner_id: &str) -> &'static str {
    if owner_id.starts_with("org_") {
        "organization"
    } else {
        "user"
    }
}

fn is_local_starter_subscription(subscription: &models::billing::Subscription) -> bool {
    subscription
        .provider_subscription_id
        .starts_with("local_starter_")
}

fn checkout_response(checkout: common::dodo::CheckoutSession) -> CheckoutOutcome {
    CheckoutOutcome {
        requires_checkout: true,
        checkout_id: Some(checkout.checkout_id),
        checkout_url: Some(checkout.checkout_url),
    }
}

async fn get_billing_account_or_404(
    state: &AppState,
    owner_id: &str,
) -> Result<BillingAccountWithSubscription, AppError> {
    GetBillingAccountQuery::new(owner_id.to_string())
        .execute_with_db(state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("Billing account not found".to_string()))
}

async fn get_plan_product_or_404(
    state: &AppState,
    plan_name: &str,
) -> Result<DodoProduct, AppError> {
    GetDodoProductQuery::new(plan_name)
        .execute_with_db(state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::NotFound("Plan not found".to_string()))
}

async fn get_pulse_product_or_500(state: &AppState) -> Result<DodoProduct, AppError> {
    GetDodoProductQuery::new("pulse_credits")
        .execute_with_db(state.db_router.writer())
        .await?
        .ok_or_else(|| AppError::Internal("Pulse product configuration missing".to_string()))
}

async fn set_provider_customer_id_with_retry(
    state: &AppState,
    owner_id: &str,
    provider_customer_id: &str,
) -> Result<(), AppError> {
    let max_attempts = 3;
    let mut last_err: Option<AppError> = None;

    for attempt in 1..=max_attempts {
        let result = async {
            let mut tx = state.db_router.writer().begin().await?;
            SetProviderCustomerIdCommand::new(
                owner_id.to_string(),
                provider_customer_id.to_string(),
            )
            .execute_with_db(&mut *tx)
            .await?;
            tx.commit().await?;
            Ok::<(), AppError>(())
        }
        .await;

        match result {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_err = Some(err);
                if attempt < max_attempts {
                    tokio::time::sleep(StdDuration::from_millis((attempt * 100) as u64)).await;
                }
            }
        }
    }

    Err(last_err.unwrap_or_else(|| {
        AppError::Internal("Failed to persist provider customer id".to_string())
    }))
}

async fn mark_checkout_session_created(
    state: &AppState,
    owner_id: &str,
    checkout_session_id: &str,
) -> Result<(), AppError> {
    let mut tx = state.db_router.writer().begin().await?;
    MarkCheckoutSessionCreatedCommand::new(owner_id.to_string(), checkout_session_id.to_string())
        .execute_with_db(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn customer_belongs_to_owner(customer: &Customer, owner_id: &str) -> bool {
    customer
        .metadata
        .as_ref()
        .and_then(|m| m.get("owner_id"))
        .and_then(|v| v.as_str())
        .map(|v| v == owner_id)
        .unwrap_or(false)
}

async fn find_existing_customer_id_by_owner_and_email(
    dodo: &DodoClient,
    owner_id: &str,
    email: &str,
) -> Result<Option<String>, AppError> {
    const PAGE_SIZE: i32 = 100;
    const MAX_PAGES: i32 = 5;

    for page_number in 0..MAX_PAGES {
        let page = dodo
            .list_customers(ListCustomersParams {
                page_size: Some(PAGE_SIZE),
                page_number: Some(page_number),
                email: Some(email),
                name: None,
                created_at_gte: None,
                created_at_lte: None,
            })
            .await
            .map_err(|e| {
                error!(
                    "Failed listing Dodo customers for owner {} and email {}: {}",
                    owner_id, email, e
                );
                AppError::Internal("Failed to reconcile customer".to_string())
            })?;

        if page.items.is_empty() {
            return Ok(None);
        }

        if let Some(customer) = page
            .items
            .iter()
            .find(|c| customer_belongs_to_owner(c, owner_id))
        {
            return Ok(Some(customer.customer_id.clone()));
        }

        if page.items.len() < PAGE_SIZE as usize {
            break;
        }
    }

    Ok(None)
}

async fn ensure_provider_customer_id(
    state: &AppState,
    dodo: &DodoClient,
    owner_id: &str,
    legal_name: &str,
    billing_email: &str,
    existing_provider_customer_id: Option<&str>,
) -> Result<String, AppError> {
    if let Some(cid) = existing_provider_customer_id.filter(|v| !v.is_empty()) {
        let _ = dodo
            .update_customer(
                cid,
                UpdateCustomerParams {
                    email: Some(billing_email.to_string()),
                    name: Some(legal_name.to_string()),
                    metadata: None,
                },
            )
            .await;

        return Ok(cid.to_string());
    }

    if let Some(recovered_customer_id) =
        find_existing_customer_id_by_owner_and_email(dodo, owner_id, billing_email).await?
    {
        set_provider_customer_id_with_retry(state, owner_id, &recovered_customer_id).await?;

        let _ = dodo
            .update_customer(
                &recovered_customer_id,
                UpdateCustomerParams {
                    email: Some(billing_email.to_string()),
                    name: Some(legal_name.to_string()),
                    metadata: None,
                },
            )
            .await;

        return Ok(recovered_customer_id);
    }

    let customer = dodo
        .create_customer(CreateCustomerParams {
            email: billing_email.to_string(),
            name: Some(legal_name.to_string()),
            metadata: Some(serde_json::json!({ "owner_id": owner_id })),
        })
        .await
        .map_err(|e| {
            error!("Failed to create Dodo customer: {}", e);
            AppError::Internal("Failed to create customer".to_string())
        })?;

    set_provider_customer_id_with_retry(state, owner_id, &customer.customer_id)
        .await
        .map_err(|err| {
            error!(
                "Created Dodo customer {} but failed to persist mapping for owner {}: {}",
                customer.customer_id, owner_id, err
            );
            AppError::Internal("Failed to persist customer mapping".to_string())
        })?;

    Ok(customer.customer_id)
}

pub async fn get_billing_account(
    state: &AppState,
    owner_id: &str,
) -> Result<Option<models::billing::BillingAccountWithSubscription>, AppError> {
    GetBillingAccountQuery::new(owner_id.to_string())
        .execute_with_db(state.db_router.writer())
        .await
}

pub async fn update_billing_account(
    state: &AppState,
    owner_id: &str,
    req: UpdateBillingAccountInput,
) -> Result<(), AppError> {
    let existing = get_billing_account_or_404(state, owner_id).await?;
    enforce_checkout_cooldown(&existing)?;

    UpdateBillingAccountCommand::new(existing.billing_account.id)
        .with_legal_name(req.legal_name)
        .with_billing_email(req.billing_email)
        .with_billing_phone(req.billing_phone)
        .with_tax_id(req.tax_id)
        .with_address_line1(req.address_line1)
        .with_address_line2(req.address_line2)
        .with_city(req.city)
        .with_state(req.state)
        .with_postal_code(req.postal_code)
        .with_country(req.country)
        .execute_with_db(state.db_router.writer())
        .await?;

    Ok(())
}

pub async fn get_portal_url(state: &AppState, owner_id: &str) -> Result<String, AppError> {
    let account = get_billing_account_or_404(state, owner_id).await?;

    let provider_customer_id = account
        .billing_account
        .provider_customer_id
        .ok_or_else(|| AppError::NotFound("Payment provider customer not found".to_string()))?;

    let dodo = DodoClient::new().map_err(|e| AppError::Internal(e.to_string()))?;

    let portal = dodo
        .create_portal_session(&provider_customer_id)
        .await
        .map_err(|e| {
            error!("Failed to create portal session: {}", e);
            AppError::Internal("Failed to create portal session".to_string())
        })?;

    Ok(portal.url)
}

pub async fn list_invoices(
    state: &AppState,
    owner_id: &str,
) -> Result<serde_json::Value, AppError> {
    let account = get_billing_account_or_404(state, owner_id).await?;
    let invoices = queries::billing::ListBillingInvoicesQuery::new(account.billing_account.id)
        .execute_with_db(state.db_router.writer())
        .await?;
    Ok(serde_json::json!({ "items": invoices }))
}

pub async fn get_current_usage(
    state: &AppState,
    owner_id: &str,
) -> Result<(Vec<queries::billing::UsageSnapshot>, String), AppError> {
    let account_with_sub = get_billing_account_or_404(state, owner_id).await?;

    let subscription = account_with_sub.subscription.ok_or_else(|| {
        AppError::NotFound("No active subscription found for this billing account".to_string())
    })?;

    let billing_period_timestamp = subscription.previous_billing_date.ok_or_else(|| {
        AppError::Internal("Subscription missing previous_billing_date".to_string())
    })?;

    let snapshots = queries::billing::GetBillingAccountUsageQuery::new(
        account_with_sub.billing_account.id,
        billing_period_timestamp,
    )
    .execute_with_db(state.db_router.writer())
    .await?;

    Ok((snapshots, billing_period_timestamp.to_rfc3339()))
}

pub async fn list_pulse_transactions(
    state: &AppState,
    owner_id: &str,
) -> Result<Vec<models::pulse_transaction::PulseTransaction>, AppError> {
    let account = get_billing_account_or_404(state, owner_id).await?;
    queries::billing::ListPulseTransactionsQuery::new(account.billing_account.id)
        .execute_with_db(state.db_router.writer())
        .await
}

pub async fn create_checkout(
    state: &AppState,
    owner_id: &str,
    req: CreateCheckoutInput,
) -> Result<CheckoutOutcome, AppError> {
    let owner_type = owner_type_from_owner_id(owner_id);
    let is_starter_plan = req.plan_name.eq_ignore_ascii_case("starter");

    let existing = GetBillingAccountQuery::new(owner_id.to_string())
        .execute_with_db(state.db_router.writer())
        .await?;

    if let Some(account) = existing.clone() {
        if let Some(ref subscription) = account.subscription {
            if subscription.status == "active" {
                return Err(AppError::Validation(
                    "Subscription already exists".to_string(),
                ));
            }
        }
        if !is_starter_plan {
            enforce_checkout_cooldown(&account)?;
        }
    }

    let dodo = DodoClient::new().map_err(|e| AppError::Internal(e.to_string()))?;

    let provider_customer_id = if let Some(ref account) = existing {
        {
            let mut tx = state.db_router.writer().begin().await?;
            UpdateBillingAccountCommand::new(account.billing_account.id)
                .with_legal_name(Some(req.legal_name.clone()))
                .with_billing_email(Some(req.billing_email.clone()))
                .with_billing_phone(req.billing_phone.clone())
                .with_tax_id(req.tax_id.clone())
                .execute_with_db(&mut *tx)
                .await?;
            tx.commit().await?;
        }

        ensure_provider_customer_id(
            state,
            &dodo,
            owner_id,
            &req.legal_name,
            &req.billing_email,
            account.billing_account.provider_customer_id.as_deref(),
        )
        .await?
    } else {
        let mut tx = state.db_router.writer().begin().await?;
        CreateBillingAccountCommand::new(
            state.sf.next_id()? as i64,
            owner_id.to_string(),
            owner_type.to_string(),
            req.legal_name.clone(),
            req.billing_email.clone(),
        )
        .with_billing_phone(req.billing_phone.clone())
        .with_tax_id(req.tax_id.clone())
        .execute_with_db(&mut *tx)
        .await?;
        tx.commit().await?;

        ensure_provider_customer_id(
            state,
            &dodo,
            owner_id,
            &req.legal_name,
            &req.billing_email,
            None,
        )
        .await?
    };

    if is_starter_plan {
        let mut tx = state.db_router.writer().begin().await?;
        let starter_product_id = GetDodoProductQuery::new("starter")
            .execute_with_db(&mut *tx)
            .await?
            .map(|p| p.product_id)
            .unwrap_or_else(|| STARTER_PRODUCT_ID_FALLBACK.to_string());

        UpsertSubscriptionCommand::new(
            state.sf.next_id()? as i64,
            owner_id.to_string(),
            provider_customer_id,
            format!("local_starter_{}", owner_id),
            "active".to_string(),
        )
        .with_product_id(Some(starter_product_id))
        .with_previous_billing_date(Some(Utc::now()))
        .execute_with_db(&mut *tx)
        .await?;

        UpdateBillingAccountStatusCommand::new(owner_id.to_string(), "active".to_string())
            .execute_with_db(&mut *tx)
            .await?;

        tx.commit().await?;

        return Ok(CheckoutOutcome {
            requires_checkout: false,
            checkout_id: None,
            checkout_url: None,
        });
    }

    let product = get_plan_product_or_404(state, &req.plan_name).await?;

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
        error!("Failed to create plan checkout session: {}", e);
        AppError::Internal("Checkout initialization failed".to_string())
    })?;
    mark_checkout_session_created(state, owner_id, &checkout.checkout_id).await?;

    Ok(checkout_response(checkout))
}

pub async fn change_plan(
    state: &AppState,
    owner_id: &str,
    req: ChangePlanInput,
) -> Result<CheckoutOutcome, AppError> {
    let account = get_billing_account_or_404(state, owner_id).await?;

    let subscription = account
        .subscription
        .as_ref()
        .ok_or_else(|| AppError::NotFound("Subscription not found".to_string()))?;

    let product = get_plan_product_or_404(state, &req.plan_name).await?;
    let dodo = DodoClient::new().map_err(|e| AppError::Internal(e.to_string()))?;

    if is_local_starter_subscription(subscription) {
        enforce_checkout_cooldown(&account)?;

        let return_url = req.return_url.ok_or_else(|| {
            AppError::Validation("return_url is required for starter upgrades".to_string())
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
                "owner_id": owner_id,
                "owner_type": account.billing_account.owner_type,
            })),
            discount_code: None,
        };

        let checkout = dodo.create_checkout_session(params).await.map_err(|e| {
            error!("Failed to create plan checkout session: {}", e);
            AppError::Internal("Checkout initialization failed".to_string())
        })?;
        mark_checkout_session_created(state, owner_id, &checkout.checkout_id).await?;
        return Ok(checkout_response(checkout));
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
        AppError::Internal("Failed to change plan".to_string())
    })?;

    Ok(CheckoutOutcome {
        requires_checkout: false,
        checkout_id: None,
        checkout_url: None,
    })
}

pub async fn create_pulse_checkout(
    state: &AppState,
    owner_id: &str,
    req: CreatePulseCheckoutInput,
) -> Result<CheckoutOutcome, AppError> {
    let existing = get_billing_account_or_404(state, owner_id).await?;

    let provider_customer_id = existing
        .billing_account
        .provider_customer_id
        .ok_or_else(|| AppError::NotFound("Payment provider customer not found".to_string()))?;

    let product = get_pulse_product_or_500(state).await?;
    let total_charge = ((req.pulse_amount + 50) as f64 / 0.96).ceil() as i64;
    let dodo = DodoClient::new().map_err(|e| AppError::Internal(e.to_string()))?;

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
        AppError::Internal("Failed to create checkout session".to_string())
    })?;
    mark_checkout_session_created(state, owner_id, &checkout.checkout_id).await?;
    Ok(checkout_response(checkout))
}
