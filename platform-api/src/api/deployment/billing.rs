use axum::{
    extract::{Json, Path, Query as QueryParams, State},
    http::StatusCode,
};
use chrono::Utc;

use crate::{
    application::{HttpState, response::ApiResult},
    core::{
        commands::{
            CreateDeploymentStripeAccountCommand,
            DeleteDeploymentStripeAccountCommand, CreateDeploymentBillingPlanCommand,
            CreateDeploymentSubscriptionCommand, Command,
        },
        dto::json::billing::{
            InitiateStripeConnectRequest, StripeConnectResponse, StripeAccountStatusResponse,
            CreateBillingPlanRequest, UpdateBillingPlanRequest, BillingPlanResponse,
            CreateSubscriptionRequest, UpdateSubscriptionRequest, SubscriptionResponse,
            BillingListQuery, BillingDashboardResponse,
        },
        models::{StripeAccountType, BillingInterval, BillingUsageType, SubscriptionStatus, CollectionMethod},
        queries::{
            GetDeploymentStripeAccountQuery, GetDeploymentBillingPlansQuery,
            GetDeploymentBillingPlanByIdQuery, GetDeploymentSubscriptionsQuery,
            GetDeploymentSubscriptionByIdQuery, Query,
        },
    },
};

// Stripe Connect Account Management
pub async fn initiate_stripe_connect(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    Json(request): Json<InitiateStripeConnectRequest>,
) -> ApiResult<StripeConnectResponse> {
    // For demo purposes, we'll create a mock Stripe account
    // In production, this would integrate with Stripe API
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let stripe_account_id = format!("acct_{}", timestamp);
    let onboarding_url = format!(
        "https://connect.stripe.com/setup/e/{}/sKJfaGFqTm8x",
        stripe_account_id
    );

    // Parse account type
    let account_type = match request.account_type.as_str() {
        "standard" => StripeAccountType::Standard,
        "express" => StripeAccountType::Express,
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid account type").into()),
    };

    // Create the Stripe account record
    let command = CreateDeploymentStripeAccountCommand {
        deployment_id,
        stripe_account_id: stripe_account_id.clone(),
        stripe_user_id: None,
        access_token_encrypted: None,
        refresh_token_encrypted: None,
        account_type,
        onboarding_url: Some(onboarding_url.clone()),
        metadata: Some(serde_json::json!({
            "refresh_url": request.refresh_url,
            "return_url": request.return_url,
        })),
    };

    command.execute(&app_state).await?;

    Ok(StripeConnectResponse {
        onboarding_url,
        account_id: stripe_account_id,
    }.into())
}

pub async fn get_stripe_account_status(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
) -> ApiResult<StripeAccountStatusResponse> {
    match GetDeploymentStripeAccountQuery::new(deployment_id)
        .execute(&app_state)
        .await
    {
        Ok(account) => {
            Ok(StripeAccountStatusResponse {
                id: account.id,
                stripe_account_id: account.stripe_account_id,
                account_type: account.account_type.to_string(),
                charges_enabled: account.charges_enabled,
                details_submitted: account.details_submitted,
                setup_completed_at: account.setup_completed_at,
                dashboard_url: account.dashboard_url,
                country: account.country,
                default_currency: account.default_currency,
                is_setup_complete: account.is_setup_complete,
            }.into())
        }
        Err(crate::core::error::AppError::NotFound(_)) => {
            // Return a 404 when no Stripe account is found
            Err((StatusCode::NOT_FOUND, "No Stripe account connected").into())
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn disconnect_stripe_account(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
) -> ApiResult<()> {
    let command = DeleteDeploymentStripeAccountCommand {
        deployment_id,
    };

    command.execute(&app_state).await?;
    
    Ok(().into())
}

pub async fn get_stripe_dashboard_url(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
) -> ApiResult<serde_json::Value> {
    let account = GetDeploymentStripeAccountQuery::new(deployment_id)
        .execute(&app_state)
        .await?;

    if let Some(dashboard_url) = account.dashboard_url {
        Ok(serde_json::json!({
            "dashboard_url": dashboard_url
        }).into())
    } else {
        Err((StatusCode::NOT_FOUND, "Dashboard URL not available").into())
    }
}

// Billing Plans Management
pub async fn get_billing_plans(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    QueryParams(query): QueryParams<BillingListQuery>,
) -> ApiResult<Vec<BillingPlanResponse>> {
    let plans = GetDeploymentBillingPlansQuery::new(deployment_id)
        .active_only(true)
        .with_limit(query.limit)
        .with_offset(query.offset)
        .execute(&app_state)
        .await?;

    let responses: Vec<BillingPlanResponse> = plans
        .into_iter()
        .map(|plan| {
            let is_trial_available = plan.is_trial_available();
            let is_metered = plan.is_metered();
            let amount = plan.amount_in_currency_unit();
            BillingPlanResponse {
                id: plan.id,
                created_at: plan.created_at,
                updated_at: plan.updated_at,
                name: plan.name,
                description: plan.description,
                stripe_price_id: plan.stripe_price_id,
                billing_interval: plan.billing_interval.to_string(),
                amount_cents: plan.amount_cents,
                amount,
                currency: plan.currency,
                trial_period_days: plan.trial_period_days,
                usage_type: plan.usage_type.clone().map(|u| u.to_string()),
                features: plan.features,
                is_active: plan.is_active,
                display_order: plan.display_order,
                is_trial_available,
                is_metered,
            }
        })
        .collect();

    Ok(responses.into())
}

pub async fn create_billing_plan(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    Json(request): Json<CreateBillingPlanRequest>,
) -> ApiResult<BillingPlanResponse> {
    // Parse billing interval
    let billing_interval = match request.billing_interval.as_str() {
        "day" => BillingInterval::Day,
        "week" => BillingInterval::Week,
        "month" => BillingInterval::Month,
        "year" => BillingInterval::Year,
        _ => return Err((StatusCode::BAD_REQUEST, "Invalid billing interval").into()),
    };

    // Parse usage type
    let usage_type = request.usage_type.as_ref().and_then(|ut| match ut.as_str() {
        "licensed" => Some(BillingUsageType::Licensed),
        "metered" => Some(BillingUsageType::Metered),
        _ => None,
    });

    // For demo purposes, generate a mock Stripe price ID
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let stripe_price_id = format!("price_{}", timestamp);

    let command = CreateDeploymentBillingPlanCommand {
        deployment_id,
        name: request.name,
        description: request.description,
        stripe_price_id: Some(stripe_price_id),
        billing_interval,
        amount_cents: request.amount_cents,
        currency: request.currency.unwrap_or_else(|| "usd".to_string()),
        trial_period_days: request.trial_period_days,
        usage_type,
        features: request.features,
        is_active: true,
        display_order: request.display_order.unwrap_or(0),
    };

    let plan = command.execute(&app_state).await?;

    let is_trial_available = plan.is_trial_available();
    let is_metered = plan.is_metered();
    let amount = plan.amount_in_currency_unit();
    
    Ok(BillingPlanResponse {
        id: plan.id,
        created_at: plan.created_at,
        updated_at: plan.updated_at,
        name: plan.name,
        description: plan.description,
        stripe_price_id: plan.stripe_price_id,
        billing_interval: plan.billing_interval.to_string(),
        amount_cents: plan.amount_cents,
        amount,
        currency: plan.currency,
        trial_period_days: plan.trial_period_days,
        usage_type: plan.usage_type.map(|u| u.to_string()),
        features: plan.features,
        is_active: plan.is_active,
        display_order: plan.display_order,
        is_trial_available,
        is_metered,
    }.into())
}

pub async fn get_billing_plan_by_id(
    State(app_state): State<HttpState>,
    Path((deployment_id, plan_id)): Path<(i64, i64)>,
) -> ApiResult<BillingPlanResponse> {
    let plan = GetDeploymentBillingPlanByIdQuery::new(deployment_id, plan_id)
        .execute(&app_state)
        .await?;

    let is_trial_available = plan.is_trial_available();
    let is_metered = plan.is_metered();
    let amount = plan.amount_in_currency_unit();
    
    Ok(BillingPlanResponse {
        id: plan.id,
        created_at: plan.created_at,
        updated_at: plan.updated_at,
        name: plan.name,
        description: plan.description,
        stripe_price_id: plan.stripe_price_id,
        billing_interval: plan.billing_interval.to_string(),
        amount_cents: plan.amount_cents,
        amount,
        currency: plan.currency,
        trial_period_days: plan.trial_period_days,
        usage_type: plan.usage_type.map(|u| u.to_string()),
        features: plan.features,
        is_active: plan.is_active,
        display_order: plan.display_order,
        is_trial_available,
        is_metered,
    }.into())
}

pub async fn update_billing_plan(
    State(_app_state): State<HttpState>,
    Path((_deployment_id, _plan_id)): Path<(i64, i64)>,
    Json(_request): Json<UpdateBillingPlanRequest>,
) -> ApiResult<BillingPlanResponse> {
    // TODO: Implement billing plan updates
    // This would involve:
    // 1. Updating plan details in deployment_billing_plans
    // 2. Optionally updating Stripe Price metadata
    // 3. Returning the updated plan
    
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented yet").into())
}

pub async fn delete_billing_plan(
    State(_app_state): State<HttpState>,
    Path((_deployment_id, _plan_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    // TODO: Implement billing plan deletion
    // This would involve:
    // 1. Deactivating the plan in deployment_billing_plans
    // 2. Handling active subscriptions
    // 3. Optionally archiving the Stripe Price
    
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented yet").into())
}

// Subscription Management
pub async fn get_subscriptions(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    QueryParams(query): QueryParams<BillingListQuery>,
) -> ApiResult<Vec<SubscriptionResponse>> {
    let mut query_builder = GetDeploymentSubscriptionsQuery::new(deployment_id)
        .with_limit(query.limit)
        .with_offset(query.offset);

    if let Some(status) = query.status {
        if let Ok(status_enum) = status.parse() {
            query_builder = query_builder.with_status(status_enum);
        }
    }

    let subscriptions = query_builder.execute(&app_state).await?;

    let responses: Vec<SubscriptionResponse> = subscriptions
        .into_iter()
        .map(|sub_with_plan| {
            let sub = sub_with_plan.subscription;
            let is_active = sub.is_active();
            let is_in_trial = sub.is_in_trial();
            let is_canceled = sub.is_canceled();
            let is_past_due = sub.is_past_due();
            let days_until_period_end = sub.days_until_period_end();
            let trial_days_remaining = sub.trial_days_remaining();
            
            SubscriptionResponse {
                id: sub.id,
                created_at: sub.created_at,
                updated_at: sub.updated_at,
                user_id: sub.user_id,
                stripe_subscription_id: sub.stripe_subscription_id,
                stripe_customer_id: sub.stripe_customer_id,
                status: sub.status.to_string(),
                current_period_start: sub.current_period_start,
                current_period_end: sub.current_period_end,
                trial_start: sub.trial_start,
                trial_end: sub.trial_end,
                cancel_at_period_end: sub.cancel_at_period_end,
                canceled_at: sub.canceled_at,
                ended_at: sub.ended_at,
                collection_method: sub.collection_method.to_string(),
                customer_email: sub.customer_email,
                customer_name: sub.customer_name,
                metadata: sub.metadata,
                billing_plan: sub_with_plan.billing_plan.map(|plan| {
                    let is_trial_available = plan.is_trial_available();
                    let is_metered = plan.is_metered();
                    let amount = plan.amount_in_currency_unit();
                    BillingPlanResponse {
                        id: plan.id,
                        created_at: plan.created_at,
                        updated_at: plan.updated_at,
                        name: plan.name,
                        description: plan.description,
                        stripe_price_id: plan.stripe_price_id,
                        billing_interval: plan.billing_interval.to_string(),
                        amount_cents: plan.amount_cents,
                        amount,
                        currency: plan.currency,
                        trial_period_days: plan.trial_period_days,
                        usage_type: plan.usage_type.clone().map(|u| u.to_string()),
                        features: plan.features,
                        is_active: plan.is_active,
                        display_order: plan.display_order,
                        is_trial_available,
                        is_metered,
                    }
                }),
                is_active,
                is_in_trial,
                is_canceled,
                is_past_due,
                days_until_period_end,
                trial_days_remaining,
            }
        })
        .collect();

    Ok(responses.into())
}

pub async fn create_subscription(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
    Json(request): Json<CreateSubscriptionRequest>,
) -> ApiResult<SubscriptionResponse> {
    // Get the billing plan to validate it exists
    let plan = GetDeploymentBillingPlanByIdQuery::new(deployment_id, request.billing_plan_id)
        .execute(&app_state)
        .await?;

    // For demo purposes, generate mock Stripe IDs
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let stripe_subscription_id = format!("sub_{}", timestamp);
    let stripe_customer_id = format!("cus_{}_{}", timestamp, deployment_id);

    // Parse collection method
    let collection_method = match request.collection_method.as_deref() {
        Some("charge_automatically") => CollectionMethod::ChargeAutomatically,
        Some("send_invoice") => CollectionMethod::SendInvoice,
        _ => CollectionMethod::ChargeAutomatically,
    };

    // Set up trial period if applicable
    let now = Utc::now();
    let (trial_start, trial_end) = if plan.trial_period_days.unwrap_or(0) > 0 {
        (Some(now), Some(now + chrono::Duration::days(plan.trial_period_days.unwrap_or(0) as i64)))
    } else {
        (None, None)
    };

    // Set current period
    let current_period_start = now;
    let current_period_end = match plan.billing_interval {
        BillingInterval::Day => now + chrono::Duration::days(1),
        BillingInterval::Week => now + chrono::Duration::weeks(1),
        BillingInterval::Month => now + chrono::Duration::days(30), // Simplified
        BillingInterval::Year => now + chrono::Duration::days(365), // Simplified
    };

    let command = CreateDeploymentSubscriptionCommand {
        deployment_id,
        billing_plan_id: plan.id,
        user_id: None, // Would be set based on authenticated user
        stripe_subscription_id: stripe_subscription_id.clone(),
        stripe_customer_id: stripe_customer_id.clone(),
        status: if trial_start.is_some() { SubscriptionStatus::Trialing } else { SubscriptionStatus::Active },
        current_period_start,
        current_period_end,
        trial_start,
        trial_end,
        collection_method,
        customer_email: request.customer_email,
        customer_name: request.customer_name,
        metadata: Some(serde_json::json!({
            "created_via": "api",
        })),
    };

    let subscription = command.execute(&app_state).await?;

    let is_active = subscription.is_active();
    let is_in_trial = subscription.is_in_trial();
    let is_canceled = subscription.is_canceled();
    let is_past_due = subscription.is_past_due();
    let days_until_period_end = subscription.days_until_period_end();
    let trial_days_remaining = subscription.trial_days_remaining();

    Ok(SubscriptionResponse {
        id: subscription.id,
        created_at: subscription.created_at,
        updated_at: subscription.updated_at,
        user_id: subscription.user_id,
        stripe_subscription_id: subscription.stripe_subscription_id,
        stripe_customer_id: subscription.stripe_customer_id,
        status: subscription.status.to_string(),
        current_period_start: subscription.current_period_start,
        current_period_end: subscription.current_period_end,
        trial_start: subscription.trial_start,
        trial_end: subscription.trial_end,
        cancel_at_period_end: subscription.cancel_at_period_end,
        canceled_at: subscription.canceled_at,
        ended_at: subscription.ended_at,
        collection_method: subscription.collection_method.to_string(),
        customer_email: subscription.customer_email,
        customer_name: subscription.customer_name,
        metadata: subscription.metadata,
        billing_plan: Some(BillingPlanResponse {
            id: plan.id,
            created_at: plan.created_at,
            updated_at: plan.updated_at,
            name: plan.name.clone(),
            description: plan.description.clone(),
            stripe_price_id: plan.stripe_price_id.clone(),
            billing_interval: plan.billing_interval.to_string(),
            amount_cents: plan.amount_cents,
            amount: plan.amount_in_currency_unit(),
            currency: plan.currency.clone(),
            trial_period_days: plan.trial_period_days,
            usage_type: plan.usage_type.clone().map(|u| u.to_string()),
            features: plan.features.clone(),
            is_active: plan.is_active,
            display_order: plan.display_order,
            is_trial_available: plan.is_trial_available(),
            is_metered: plan.is_metered(),
        }),
        is_active,
        is_in_trial,
        is_canceled,
        is_past_due,
        days_until_period_end,
        trial_days_remaining,
    }.into())
}

pub async fn get_subscription_by_id(
    State(app_state): State<HttpState>,
    Path((deployment_id, subscription_id)): Path<(i64, i64)>,
) -> ApiResult<SubscriptionResponse> {
    let subscription = GetDeploymentSubscriptionByIdQuery::new(deployment_id, subscription_id)
        .execute(&app_state)
        .await?;

    let is_active = subscription.is_active();
    let is_in_trial = subscription.is_in_trial();
    let is_canceled = subscription.is_canceled();
    let is_past_due = subscription.is_past_due();
    let days_until_period_end = subscription.days_until_period_end();
    let trial_days_remaining = subscription.trial_days_remaining();

    Ok(SubscriptionResponse {
        id: subscription.id,
        created_at: subscription.created_at,
        updated_at: subscription.updated_at,
        user_id: subscription.user_id,
        stripe_subscription_id: subscription.stripe_subscription_id,
        stripe_customer_id: subscription.stripe_customer_id,
        status: subscription.status.to_string(),
        current_period_start: subscription.current_period_start,
        current_period_end: subscription.current_period_end,
        trial_start: subscription.trial_start,
        trial_end: subscription.trial_end,
        cancel_at_period_end: subscription.cancel_at_period_end,
        canceled_at: subscription.canceled_at,
        ended_at: subscription.ended_at,
        collection_method: subscription.collection_method.to_string(),
        customer_email: subscription.customer_email,
        customer_name: subscription.customer_name,
        metadata: subscription.metadata,
        billing_plan: None, // TODO: Load associated billing plan
        is_active,
        is_in_trial,
        is_canceled,
        is_past_due,
        days_until_period_end,
        trial_days_remaining,
    }.into())
}

pub async fn update_subscription(
    State(_app_state): State<HttpState>,
    Path((_deployment_id, _subscription_id)): Path<(i64, i64)>,
    Json(_request): Json<UpdateSubscriptionRequest>,
) -> ApiResult<SubscriptionResponse> {
    // TODO: Implement subscription updates
    // This would involve:
    // 1. Updating Stripe Subscription
    // 2. Updating subscription details in deployment_subscriptions
    // 3. Returning the updated subscription
    
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented yet").into())
}

pub async fn cancel_subscription(
    State(_app_state): State<HttpState>,
    Path((_deployment_id, _subscription_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    // TODO: Implement subscription cancellation
    // This would involve:
    // 1. Canceling Stripe Subscription
    // 2. Updating subscription status in deployment_subscriptions
    
    Err((StatusCode::NOT_IMPLEMENTED, "Not implemented yet").into())
}

// Billing Dashboard
pub async fn get_billing_dashboard(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
) -> ApiResult<BillingDashboardResponse> {
    // Get basic data for the dashboard
    // In a full implementation, this would aggregate more data
    
    // Try to get Stripe account (might not exist)
    let stripe_account = GetDeploymentStripeAccountQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .ok();

    // Get billing plans
    let billing_plans = GetDeploymentBillingPlansQuery::new(deployment_id)
        .active_only(true)
        .execute(&app_state)
        .await
        .unwrap_or_default();

    // Get active subscriptions
    let active_subscriptions = GetDeploymentSubscriptionsQuery::new(deployment_id)
        .with_status(SubscriptionStatus::Active)
        .execute(&app_state)
        .await
        .unwrap_or_default();

    // Mock response with basic data
    Ok(BillingDashboardResponse {
        stripe_account: stripe_account.map(|acc| StripeAccountStatusResponse {
            id: acc.id,
            stripe_account_id: acc.stripe_account_id,
            account_type: acc.account_type.to_string(),
            charges_enabled: acc.charges_enabled,
            details_submitted: acc.details_submitted,
            setup_completed_at: acc.setup_completed_at,
            dashboard_url: acc.dashboard_url,
            country: acc.country,
            default_currency: acc.default_currency,
            is_setup_complete: acc.is_setup_complete,
        }),
        active_subscriptions: active_subscriptions.into_iter().map(|sub_with_plan| {
            let sub = sub_with_plan.subscription;
            let is_active = sub.is_active();
            let is_in_trial = sub.is_in_trial();
            let is_canceled = sub.is_canceled();
            let is_past_due = sub.is_past_due();
            let days_until_period_end = sub.days_until_period_end();
            let trial_days_remaining = sub.trial_days_remaining();
            
            SubscriptionResponse {
                id: sub.id,
                created_at: sub.created_at,
                updated_at: sub.updated_at,
                user_id: sub.user_id,
                stripe_subscription_id: sub.stripe_subscription_id,
                stripe_customer_id: sub.stripe_customer_id,
                status: sub.status.to_string(),
                current_period_start: sub.current_period_start,
                current_period_end: sub.current_period_end,
                trial_start: sub.trial_start,
                trial_end: sub.trial_end,
                cancel_at_period_end: sub.cancel_at_period_end,
                canceled_at: sub.canceled_at,
                ended_at: sub.ended_at,
                collection_method: sub.collection_method.to_string(),
                customer_email: sub.customer_email,
                customer_name: sub.customer_name,
                metadata: sub.metadata,
                billing_plan: sub_with_plan.billing_plan.map(|plan| {
                    let is_trial_available = plan.is_trial_available();
                    let is_metered = plan.is_metered();
                    let amount = plan.amount_in_currency_unit();
                    
                    BillingPlanResponse {
                        id: plan.id,
                        created_at: plan.created_at,
                        updated_at: plan.updated_at,
                        name: plan.name,
                        description: plan.description,
                        stripe_price_id: plan.stripe_price_id,
                        billing_interval: plan.billing_interval.to_string(),
                        amount_cents: plan.amount_cents,
                        amount,
                        currency: plan.currency,
                        trial_period_days: plan.trial_period_days,
                        usage_type: plan.usage_type.map(|u| u.to_string()),
                        features: plan.features,
                        is_active: plan.is_active,
                        display_order: plan.display_order,
                        is_trial_available,
                        is_metered,
                    }
                }),
                is_active,
                is_in_trial,
                is_canceled,
                is_past_due,
                days_until_period_end,
                trial_days_remaining,
            }
        }).collect(),
        recent_invoices: vec![], // Would fetch from deployment_invoices table
        billing_plans: billing_plans.into_iter().map(|plan| {
            let is_trial_available = plan.is_trial_available();
            let is_metered = plan.is_metered();
            let amount = plan.amount_in_currency_unit();
            
            BillingPlanResponse {
                id: plan.id,
                created_at: plan.created_at,
                updated_at: plan.updated_at,
                name: plan.name,
                description: plan.description,
                stripe_price_id: plan.stripe_price_id,
                billing_interval: plan.billing_interval.to_string(),
                amount_cents: plan.amount_cents,
                amount,
                currency: plan.currency,
                trial_period_days: plan.trial_period_days,
                usage_type: plan.usage_type.map(|u| u.to_string()),
                features: plan.features,
                is_active: plan.is_active,
                display_order: plan.display_order,
                is_trial_available,
                is_metered,
            }
        }).collect(),
        payment_methods: vec![], // Would fetch from deployment_payment_methods table
        usage_summary: None, // Would calculate from deployment_usage_records table
    }.into())
}