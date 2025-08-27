use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};

use common::state::AppState;
use common::chargebee::{
    ChargebeeClient, CreateCheckoutParams, CheckoutSubscription, CustomerInfo,
};
use models::billing::Subscription;
use queries::{
    Query as QueryTrait,
    billing::{GetUserSubscriptionQuery, GetOrganizationSubscriptionQuery},
};
use commands::{
    Command,
    billing::UpdateSubscriptionStatusCommand,
};

#[derive(Debug, Deserialize)]
pub struct CreateCheckoutRequest {
    pub plan_id: String,
    pub email: String,
    pub name: Option<String>,
    pub user_id: Option<i64>,
    pub organization_id: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CheckoutResponse {
    pub checkout_url: String,
}

#[derive(Debug, Serialize)]
pub struct PortalResponse {
    pub portal_url: String,
}

// Get subscription status for a specific user (admin endpoint)
pub async fn get_user_subscription(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<Option<Subscription>>, StatusCode> {
    let subscription = GetUserSubscriptionQuery::new(user_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(subscription))
}

// Get subscription status for a specific organization
pub async fn get_organization_subscription(
    State(state): State<AppState>,
    Path(org_id): Path<i64>,
) -> Result<Json<Option<Subscription>>, StatusCode> {
    let subscription = GetOrganizationSubscriptionQuery::new(org_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(subscription))
}

// Create checkout session for new subscription
pub async fn create_checkout(
    State(state): State<AppState>,
    Json(req): Json<CreateCheckoutRequest>,
) -> Result<Json<CheckoutResponse>, StatusCode> {
    // Validate that either user_id or organization_id is provided
    let (entity_type, entity_id) = match (req.user_id, req.organization_id) {
        (Some(uid), None) => ("user", uid),
        (None, Some(oid)) => ("org", oid),
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    
    // Check if already has subscription
    let existing = if entity_type == "user" {
        GetUserSubscriptionQuery::new(entity_id)
            .execute(&state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    } else {
        GetOrganizationSubscriptionQuery::new(entity_id)
            .execute(&state)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };
    
    if existing.is_some() {
        return Err(StatusCode::CONFLICT);
    }
    
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let params = CreateCheckoutParams {
        subscription: CheckoutSubscription {
            plan_id: req.plan_id,
            trial_end: None,
        },
        customer: CustomerInfo {
            id: Some(format!("{}_{}", entity_type, entity_id)),
            email: req.email,
            first_name: req.name,
            last_name: None,
            company: None,
        },
        redirect_url: Some(format!("{}/billing/success", std::env::var("APP_URL").unwrap_or_default())),
        cancel_url: Some(format!("{}/billing", std::env::var("APP_URL").unwrap_or_default())),
    };
    
    let response = chargebee.create_checkout_session(params)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    let url = response["hosted_page"]["url"]
        .as_str()
        .unwrap_or("")
        .to_string();
    
    Ok(Json(CheckoutResponse {
        checkout_url: url,
    }))
}

// Get customer portal URL for user
pub async fn get_user_portal_url(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<PortalResponse>, StatusCode> {
    let subscription = GetUserSubscriptionQuery::new(user_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
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

// Get customer portal URL for organization
pub async fn get_org_portal_url(
    State(state): State<AppState>,
    Path(org_id): Path<i64>,
) -> Result<Json<PortalResponse>, StatusCode> {
    let subscription = GetOrganizationSubscriptionQuery::new(org_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
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

// Cancel user subscription
pub async fn cancel_user_subscription(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let subscription = GetUserSubscriptionQuery::new(user_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Cancel in Chargebee
    chargebee.cancel_subscription(&subscription.chargebee_subscription_id, true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Update local status
    UpdateSubscriptionStatusCommand {
        subscription_id: subscription.id,
        status: "cancelled".to_string(),
    }
    .execute(&state)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(StatusCode::OK)
}

// Cancel organization subscription
pub async fn cancel_org_subscription(
    State(state): State<AppState>,
    Path(org_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let subscription = GetOrganizationSubscriptionQuery::new(org_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Cancel in Chargebee
    chargebee.cancel_subscription(&subscription.chargebee_subscription_id, true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // Update local status
    UpdateSubscriptionStatusCommand {
        subscription_id: subscription.id,
        status: "cancelled".to_string(),
    }
    .execute(&state)
    .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(StatusCode::OK)
}