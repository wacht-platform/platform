use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
};
use serde::{Deserialize, Serialize};

use crate::application::HttpState;
use common::chargebee::{
    ChargebeeClient, CreateCheckoutParams, CheckoutSubscription, CustomerInfo,
};
use models::billing::Subscription;
use queries::{
    Query as QueryTrait,
    billing::GetProjectSubscriptionQuery,
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
}

#[derive(Debug, Serialize)]
pub struct CheckoutResponse {
    pub checkout_url: String,
}

#[derive(Debug, Serialize)]
pub struct PortalResponse {
    pub portal_url: String,
}

// Get subscription status for a project
pub async fn get_subscription(
    State(state): State<HttpState>,
    Path(project_id): Path<i64>,
) -> Result<Json<Option<Subscription>>, StatusCode> {
    let subscription = GetProjectSubscriptionQuery::new(project_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    Ok(Json(subscription))
}

// Create checkout session for new subscription
pub async fn create_checkout(
    State(state): State<HttpState>,
    Path(project_id): Path<i64>,
    Json(req): Json<CreateCheckoutRequest>,
) -> Result<Json<CheckoutResponse>, StatusCode> {
    // Check if already has subscription
    let existing = GetProjectSubscriptionQuery::new(project_id)
        .execute(&state)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
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
            id: Some(format!("project_{}", project_id)),
            email: req.email,
            first_name: req.name.clone(),
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

// Get customer portal URL
pub async fn get_portal_url(
    State(state): State<HttpState>,
    Path(project_id): Path<i64>,
) -> Result<Json<PortalResponse>, StatusCode> {
    let subscription = GetProjectSubscriptionQuery::new(project_id)
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

// Cancel subscription
pub async fn cancel_subscription(
    State(state): State<HttpState>,
    Path(project_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let subscription = GetProjectSubscriptionQuery::new(project_id)
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