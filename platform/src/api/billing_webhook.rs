use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};
use common::chargebee::ChargebeeClient;
use commands::{
    Command,
    billing::UpsertSubscriptionCommand,
};
use common::state::AppState;

// Webhook endpoint for Chargebee events
pub async fn handle_chargebee_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, StatusCode> {
    // Verify webhook signature
    let signature = headers
        .get("X-Chargebee-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    
    let chargebee = ChargebeeClient::new()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    if !chargebee.verify_webhook_signature(&body, signature) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    // Parse webhook event
    let event: serde_json::Value = serde_json::from_str(&body)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let event_type = event["event_type"].as_str().unwrap_or("");
    
    // Handle subscription events
    match event_type {
        "subscription_created" | "subscription_changed" | "subscription_cancelled" | "subscription_reactivated" => {
            if let Some(subscription) = event["content"]["subscription"].as_object() {
                if let Some(customer) = event["content"]["customer"].as_object() {
                    let customer_id = customer["id"].as_str().unwrap_or("");
                    
                    // Extract user_id or org_id from customer_id (format: "user_123" or "org_123")
                    let (user_id, org_id) = if let Some(user_id_str) = customer_id.strip_prefix("user_") {
                        (user_id_str.parse::<i64>().ok(), None)
                    } else if let Some(org_id_str) = customer_id.strip_prefix("org_") {
                        (None, org_id_str.parse::<i64>().ok())
                    } else {
                        (None, None)
                    };
                    
                    if user_id.is_some() || org_id.is_some() {
                        let status = subscription["status"].as_str().unwrap_or("active");
                        
                        UpsertSubscriptionCommand {
                            user_id,
                            organization_id: org_id,
                            chargebee_customer_id: customer_id.to_string(),
                            chargebee_subscription_id: subscription["id"].as_str().unwrap_or("").to_string(),
                            status: status.to_string(),
                        }
                        .execute(&state)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                    }
                }
            }
        }
        _ => {
            // Ignore other events
        }
    }
    
    Ok(StatusCode::OK)
}