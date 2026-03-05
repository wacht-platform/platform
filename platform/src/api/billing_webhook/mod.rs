use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};
use commands::{
    Command,
    billing::{
        MarkCheckoutFlowFailedCommand, MarkPaymentSucceededCommand,
        MarkSubscriptionActivatedCommand, UpdateBillingAccountStatusCommand, UpsertInvoiceCommand,
        UpsertSubscriptionCommand,
    },
    email::SendRawEmailCommand,
    pulse::AddPulseCreditsCommand,
};
use common::dodo::DodoClient;
use common::state::AppState;
use models::pulse_transaction::PulseTransactionType;
use queries::{
    Query,
    billing::{GetBillingAccountByProviderCustomerIdQuery, GetBillingAccountQuery},
};
use std::collections::HashSet;
use tracing::{error, info, warn};

mod notifications;
mod payment_events;
mod subscription_events;

pub async fn handle_dodo_webhook(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, StatusCode> {
    let webhook_id = headers
        .get("webhook-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let webhook_signature = headers
        .get("webhook-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let webhook_timestamp = headers
        .get("webhook-timestamp")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let dodo = DodoClient::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !dodo.verify_webhook(webhook_id, webhook_timestamp, &body, webhook_signature) {
        warn!(
            "Invalid webhook signature for webhook_id: {}. Timestamp: {}, Signature: {}, Body length: {}",
            webhook_id,
            webhook_timestamp,
            webhook_signature,
            body.len()
        );
        return Err(StatusCode::UNAUTHORIZED);
    }

    let event: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let event_type = event["type"].as_str().unwrap_or("");
    let data = &event["data"];

    info!("Received Dodo webhook: {} (id: {})", event_type, webhook_id);

    match event_type {
        "subscription.active" => {
            subscription_events::handle_subscription_active(&app_state, data).await?;
        }
        "subscription.renewed" => {
            subscription_events::handle_subscription_renewed(&app_state, data).await?;
        }
        "subscription.plan_changed" => {
            subscription_events::handle_subscription_plan_changed(&app_state, data).await?;
        }
        "subscription.cancelled" => {
            subscription_events::handle_subscription_cancelled(&app_state, data).await?;
        }
        "subscription.on_hold" => {
            subscription_events::handle_subscription_on_hold(&app_state, data).await?;
        }
        "subscription.failed" => {
            subscription_events::handle_subscription_failed(&app_state, data).await?;
        }
        "subscription.expired" => {
            subscription_events::handle_subscription_expired(&app_state, data).await?;
        }
        "payment.succeeded" => {
            payment_events::handle_payment_succeeded(&app_state, data).await?;
        }
        "payment.failed" => {
            payment_events::handle_payment_failed(&app_state, data).await?;
        }
        _ => {
            info!("Received unhandled webhook event: {}", event_type);
        }
    }

    Ok(StatusCode::OK)
}

pub(super) fn get_customer_id(data: &serde_json::Value) -> &str {
    data["customer"]["customer_id"].as_str().unwrap_or("")
}
