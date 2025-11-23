use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};
use commands::{
    Command,
    billing::{UpdateBillingAccountStatusCommand, UpsertSubscriptionCommand, UpsertInvoiceCommand},
};
use common::dodo::DodoClient;
use common::state::AppState;
use queries::{Query, billing::GetBillingAccountByProviderCustomerIdQuery};
use tracing::{error, info, warn};

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

    if !dodo.verify_webhook(
        &body,
        webhook_signature,
        webhook_timestamp,
    ) {
        warn!("Invalid webhook signature for webhook_id: {}", webhook_id);
        return Err(StatusCode::UNAUTHORIZED);
    }

    let event: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let event_type = event["type"].as_str().unwrap_or("");
    let data = &event["data"];

    info!("Received Dodo webhook: {} (id: {})", event_type, webhook_id);

    match event_type {
        "subscription.active" => {
            handle_subscription_active(&app_state, data).await?;
        }
        "subscription.renewed" => {
            handle_subscription_renewed(&app_state, data).await?;
        }
        "subscription.plan_changed" => {
            handle_subscription_plan_changed(&app_state, data).await?;
        }
        "subscription.cancelled" => {
            handle_subscription_cancelled(&app_state, data).await?;
        }
        "subscription.on_hold" => {
            handle_subscription_on_hold(&app_state, data).await?;
        }
        "subscription.failed" => {
            handle_subscription_failed(&app_state, data).await?;
        }
        "subscription.expired" => {
            handle_subscription_expired(&app_state, data).await?;
        }
        "payment.succeeded" => {
            handle_payment_succeeded(&app_state, data).await?;
        }
        "payment.failed" => {
            handle_payment_failed(&app_state, data).await?;
        }
        _ => {
            info!("Received unhandled webhook event: {}", event_type);
        }
    }

    Ok(StatusCode::OK)
}

fn get_customer_id(data: &serde_json::Value) -> &str {
    data["customer"]["customer_id"].as_str().unwrap_or("")
}

async fn handle_subscription_active(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let status = data["status"].as_str().unwrap_or("active");

    if customer_id.is_empty() || subscription_id.is_empty() {
        warn!("Missing customer_id or subscription_id in subscription webhook");
        return Ok(());
    }

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if owner_id.is_empty() {
        warn!("Could not determine owner_id from customer_id: {}", customer_id);
        return Ok(());
    }

    UpsertSubscriptionCommand {
        owner_id: owner_id.clone(),
        provider_customer_id: customer_id.to_string(),
        provider_subscription_id: subscription_id.to_string(),
        status: status.to_string(),
    }
    .execute(app_state)
    .await
    .map_err(|e| {
        error!("Failed to upsert subscription: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    UpdateBillingAccountStatusCommand {
        owner_id: owner_id.clone(),
        status: "active".to_string(),
    }
    .execute(app_state)
    .await
    .map_err(|e| {
        error!("Failed to update billing account status: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    info!("Subscription {} activated for owner {}", subscription_id, owner_id);

    Ok(())
}

async fn handle_subscription_renewed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer_id.to_string(),
            provider_subscription_id: subscription_id.to_string(),
            status: "active".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update subscription on renewal: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Subscription {} renewed for owner {}", subscription_id, owner_id);
    }

    Ok(())
}

async fn handle_subscription_plan_changed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let new_product_id = data["product_id"].as_str().unwrap_or("");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    info!(
        "Plan changed for subscription {} to product {} (owner: {})",
        subscription_id, new_product_id, owner_id
    );

    Ok(())
}

async fn handle_subscription_cancelled(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer_id.to_string(),
            provider_subscription_id: subscription_id.to_string(),
            status: "cancelled".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand {
            owner_id: owner_id.clone(),
            status: "cancelled".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Subscription {} cancelled for owner {}", subscription_id, owner_id);
    }

    Ok(())
}

async fn handle_subscription_on_hold(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer_id.to_string(),
            provider_subscription_id: subscription_id.to_string(),
            status: "on_hold".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand {
            owner_id: owner_id.clone(),
            status: "paused".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Subscription {} on hold for owner {}", subscription_id, owner_id);
    }

    Ok(())
}

async fn handle_subscription_failed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer_id.to_string(),
            provider_subscription_id: subscription_id.to_string(),
            status: "failed".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand {
            owner_id: owner_id.clone(),
            status: "failed".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Subscription {} failed for owner {}", subscription_id, owner_id);
    }

    Ok(())
}

async fn handle_subscription_expired(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpsertSubscriptionCommand {
            owner_id: owner_id.clone(),
            provider_customer_id: customer_id.to_string(),
            provider_subscription_id: subscription_id.to_string(),
            status: "expired".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update subscription status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpdateBillingAccountStatusCommand {
            owner_id: owner_id.clone(),
            status: "expired".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update billing account status: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Subscription {} expired for owner {}", subscription_id, owner_id);
    }

    Ok(())
}

async fn handle_payment_succeeded(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let payment_id = data["payment_id"].as_str().unwrap_or("");
    let amount = data["total_amount"].as_i64().unwrap_or(0);
    let currency = data["currency"].as_str().unwrap_or("USD");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpdateBillingAccountStatusCommand {
            owner_id: owner_id.clone(),
            status: "active".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update billing account status on payment success: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        UpsertInvoiceCommand {
            owner_id: owner_id.clone(),
            provider_payment_id: payment_id.to_string(),
            provider_customer_id: customer_id.to_string(),
            amount_due_cents: amount,
            amount_paid_cents: amount,
            currency: currency.to_string(),
            status: "paid".to_string(),
            invoice_pdf_url: data["payment_link"].as_str().map(|s| s.to_string()),
            hosted_invoice_url: None,
            invoice_number: None,
            due_date: None,
            paid_at: Some(chrono::Utc::now()),
            period_start: None,
            period_end: None,
            metadata: serde_json::json!({}),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to upsert invoice: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Payment {} succeeded for owner {}", payment_id, owner_id);
    }

    Ok(())
}

async fn handle_payment_failed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let payment_id = data["payment_id"].as_str().unwrap_or("");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;

    if !owner_id.is_empty() {
        UpdateBillingAccountStatusCommand {
            owner_id: owner_id.clone(),
            status: "payment_failed".to_string(),
        }
        .execute(app_state)
        .await
        .map_err(|e| {
            error!("Failed to update billing account status on payment failure: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Payment {} failed for owner {}", payment_id, owner_id);
    }

    Ok(())
}

async fn extract_owner_id(app_state: &AppState, customer_id: &str, data: &serde_json::Value) -> String {
    if let Some(metadata) = data["metadata"].as_object() {
        if let Some(owner_id) = metadata.get("owner_id").and_then(|v| v.as_str()) {
            return owner_id.to_string();
        }
    }

    if let Some(customer_metadata) = data["customer"]["metadata"].as_object() {
        if let Some(owner_id) = customer_metadata.get("owner_id").and_then(|v| v.as_str()) {
            return owner_id.to_string();
        }
    }

    if !customer_id.is_empty() {
        if let Ok(Some(owner_id)) = GetBillingAccountByProviderCustomerIdQuery::new(customer_id)
            .execute(app_state)
            .await
        {
            return owner_id;
        }
    }

    String::new()
}
