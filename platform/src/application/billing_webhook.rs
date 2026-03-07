use std::collections::HashSet;

use axum::http::{HeaderMap, StatusCode};
use commands::{
    billing::{
        MarkCheckoutFlowFailedCommand, MarkPaymentSucceededCommand,
        MarkSubscriptionActivatedCommand, UpdateBillingAccountStatusCommand, UpsertInvoiceCommand,
        UpsertSubscriptionCommand,
    },
    email::SendRawEmailCommand,
    pulse::AddPulseCreditsCommand,
};
use common::{db_router::ReadConsistency, dodo::DodoClient, state::AppState};
use models::pulse_transaction::PulseTransactionType;
use queries::billing::{GetBillingAccountByProviderCustomerIdQuery, GetBillingAccountQuery};
use tracing::{error, info, warn};

pub async fn handle_dodo_webhook(
    app_state: &AppState,
    headers: &HeaderMap,
    body: &str,
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

    if !dodo.verify_webhook(webhook_id, webhook_timestamp, body, webhook_signature) {
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
        serde_json::from_str(body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let event_type = event["type"].as_str().unwrap_or("");
    let data = &event["data"];

    info!("Received Dodo webhook: {} (id: {})", event_type, webhook_id);

    match event_type {
        "subscription.active" => handle_subscription_active(app_state, data).await?,
        "subscription.renewed" => handle_subscription_renewed(app_state, data).await?,
        "subscription.plan_changed" => handle_subscription_plan_changed(app_state, data).await?,
        "subscription.cancelled" => handle_subscription_cancelled(app_state, data).await?,
        "subscription.on_hold" => handle_subscription_on_hold(app_state, data).await?,
        "subscription.failed" => handle_subscription_failed(app_state, data).await?,
        "subscription.expired" => handle_subscription_expired(app_state, data).await?,
        "payment.succeeded" => handle_payment_succeeded(app_state, data).await?,
        "payment.failed" => handle_payment_failed(app_state, data).await?,
        _ => info!("Received unhandled webhook event: {}", event_type),
    }

    Ok(StatusCode::OK)
}

fn get_customer_id(data: &serde_json::Value) -> &str {
    data["customer"]["customer_id"].as_str().unwrap_or("")
}

fn parse_console_deployment_id() -> Option<i64> {
    std::env::var("CONSOLE_DEPLOYMENT_ID")
        .ok()
        .and_then(|v| v.parse::<i64>().ok())
}

fn split_recipients(raw: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    raw.split([',', ';'])
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .filter(|email| seen.insert(email.clone()))
        .collect()
}

async fn send_billing_change_email(app_state: &AppState, owner_id: &str, message: &str) {
    let Some(console_deployment_id) = parse_console_deployment_id() else {
        warn!("CONSOLE_DEPLOYMENT_ID not set; skipping billing change email");
        return;
    };

    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let account = match GetBillingAccountQuery::new(owner_id.to_string())
        .execute_with(reader)
        .await
    {
        Ok(Some(account)) => account,
        Ok(None) => return,
        Err(e) => {
            warn!(
                "Failed to load billing account for {} while sending billing email: {}",
                owner_id, e
            );
            return;
        }
    };

    let recipients = split_recipients(&account.billing_account.billing_email);
    if recipients.is_empty() {
        return;
    }

    let plan_line = account
        .subscription
        .as_ref()
        .and_then(|s| s.plan_name.as_ref())
        .map(|name| format!("Current plan: {}.", name));

    let mut lines = vec![message.to_string()];
    if let Some(plan_line) = plan_line {
        lines.push(plan_line);
    }
    lines.push(
        "You are receiving this email because this email is attached to your Wacht billing account."
            .to_string(),
    );

    let final_message = lines.join("\n");
    let subject = "Billing update".to_string();
    let body_html_lines = lines
        .iter()
        .map(|line| {
            format!(
                "<p style=\"font-size:16px;line-height:1.6;margin:0 0 10px 0;\">{}</p>",
                line
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let body_html = format!("<div>{}</div>", body_html_lines);
    let body_text = final_message.clone();

    for email in recipients {
        let send_email_command = SendRawEmailCommand::new(
            console_deployment_id,
            email.clone(),
            subject.clone(),
            body_html.clone(),
            Some(body_text.clone()),
        );
        if let Err(e) = send_email_command.execute_with_deps(app_state).await {
            warn!(
                "Failed to send billing change email to {} for {}: {}",
                email, owner_id, e
            );
        }
    }
}

async fn extract_owner_id(
    app_state: &AppState,
    customer_id: &str,
    data: &serde_json::Value,
) -> String {
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
        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        if let Ok(Some(owner_id)) = GetBillingAccountByProviderCustomerIdQuery::new(customer_id)
            .execute_with(reader)
            .await
        {
            return owner_id;
        }
    }

    String::new()
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
        let is_pulse_purchase = data["metadata"]
            .as_object()
            .and_then(|metadata| metadata.get("type"))
            .and_then(|v| v.as_str())
            == Some("pulse_purchase");

        if !is_pulse_purchase {
            let reader = app_state.db_router.reader(ReadConsistency::Strong);
            let status = match GetBillingAccountQuery::new(owner_id.clone())
                .execute_with(reader)
                .await
            {
                Ok(Some(account))
                    if account
                        .subscription
                        .as_ref()
                        .map(|s| s.status.eq_ignore_ascii_case("active"))
                        .unwrap_or(false) =>
                {
                    "active"
                }
                Ok(_) => "pending",
                Err(e) => {
                    error!(
                        "Failed to load billing account to determine post-payment status: {}",
                        e
                    );
                    "pending"
                }
            };

            UpdateBillingAccountStatusCommand::new(owner_id.clone(), status.to_string())
            .execute_with(app_state.db_router.writer())
            .await
            .map_err(|e| {
                error!(
                    "Failed to update billing account status on payment success: {}",
                    e
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            MarkPaymentSucceededCommand::new(
                owner_id.clone(),
                "payment.succeeded".to_string(),
            )
            .execute_with(app_state.db_router.writer())
            .await
            .map_err(|e| {
                error!("Failed to update checkout flow state: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        }

        UpsertInvoiceCommand::new(
            app_state.sf.next_id().map_err(|e| {
                error!("Failed to generate billing invoice id: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })? as i64,
            owner_id.clone(),
            payment_id.to_string(),
            customer_id.to_string(),
            amount,
            amount,
            currency.to_string(),
            "paid".to_string(),
        )
        .with_invoice_pdf_url(Some(format!(
            "https://live.dodopayments.com/invoices/payments/{}",
            payment_id
        )))
        .with_paid_at(Some(chrono::Utc::now()))
        .with_metadata(serde_json::json!({}))
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to upsert invoice: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Payment {} succeeded for owner {}", payment_id, owner_id);
        send_billing_change_email(app_state, &owner_id, "Your payment was successful.").await;

        if let Some(metadata) = data["metadata"].as_object() {
            if metadata.get("type").and_then(|v| v.as_str()) == Some("pulse_purchase") {
                let pulse_to_add = ((amount as f64 * 0.96) - 50.0).floor() as i64;

                if pulse_to_add > 0 {
                    AddPulseCreditsCommand::new(
                        owner_id.clone(),
                        pulse_to_add,
                        PulseTransactionType::Purchase,
                    )
                    .with_reference_id(Some(payment_id.to_string()))
                    .with_transaction_id(app_state.sf.next_id().map_err(|e| {
                        error!("Failed to generate pulse transaction id: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })? as i64)
                    .execute_with(app_state.db_router.writer())
                    .await
                    .map_err(|e| {
                        error!("Failed to add Pulse credits from webhook: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;
                } else {
                    warn!(
                        "Pulse purchase amount {} too low to add credits for owner {}",
                        amount, owner_id
                    );
                }
            }
        }
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
        UpdateBillingAccountStatusCommand::new(owner_id.clone(), "payment_failed".to_string())
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!(
                "Failed to update billing account status on payment failure: {}",
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkCheckoutFlowFailedCommand::new(
            owner_id.clone(),
            "payment.failed".to_string(),
            "payment_failed".to_string(),
        )
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        info!("Payment {} failed for owner {}", payment_id, owner_id);
        send_billing_change_email(
            app_state,
            &owner_id,
            "Your payment failed. Please retry your payment method.",
        )
        .await;
    }

    Ok(())
}

async fn upsert_subscription_and_status(
    app_state: &AppState,
    owner_id: &str,
    customer_id: &str,
    subscription_id: &str,
    product_id: Option<String>,
    status: &str,
    previous_billing_date: Option<chrono::DateTime<chrono::Utc>>,
    webhook_event: &str,
    send_email_message: &str,
) -> Result<(), StatusCode> {
    UpsertSubscriptionCommand::new(
        app_state.sf.next_id().map_err(|e| {
            error!("Failed to generate subscription id: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })? as i64,
        owner_id.to_string(),
        customer_id.to_string(),
        subscription_id.to_string(),
        status.to_string(),
    )
    .with_product_id(product_id)
    .with_previous_billing_date(previous_billing_date)
    .execute_with(app_state.db_router.writer())
    .await
    .map_err(|e| {
        error!("Failed to update subscription status: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    UpdateBillingAccountStatusCommand::new(owner_id.to_string(), status.to_string())
    .execute_with(app_state.db_router.writer())
    .await
    .map_err(|e| {
        error!("Failed to update billing account status: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    if status == "active" {
        MarkSubscriptionActivatedCommand::new(owner_id.to_string(), webhook_event.to_string())
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    } else {
        MarkCheckoutFlowFailedCommand::new(
            owner_id.to_string(),
            webhook_event.to_string(),
            status.to_string(),
        )
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!("Failed to update checkout flow state: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    }

    send_billing_change_email(app_state, owner_id, send_email_message).await;
    Ok(())
}

fn parse_previous_billing_date(data: &serde_json::Value) -> Option<chrono::DateTime<chrono::Utc>> {
    data["previous_billing_date"]
        .as_str()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

async fn handle_subscription_active(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let status = data["status"].as_str().unwrap_or("active");
    let previous_billing_date = parse_previous_billing_date(data);

    if customer_id.is_empty() || subscription_id.is_empty() {
        warn!("Missing customer_id or subscription_id in subscription webhook");
        return Ok(());
    }

    let owner_id = extract_owner_id(app_state, customer_id, data).await;
    if owner_id.is_empty() {
        warn!(
            "Could not determine owner_id from customer_id: {}",
            customer_id
        );
        return Ok(());
    }

    upsert_subscription_and_status(
        app_state,
        &owner_id,
        customer_id,
        subscription_id,
        product_id,
        status,
        previous_billing_date,
        "subscription.active",
        "Your subscription is now active.",
    )
    .await?;

    info!(
        "Subscription {} activated for owner {}",
        subscription_id, owner_id
    );
    Ok(())
}

async fn handle_subscription_renewed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = parse_previous_billing_date(data);

    let owner_id = extract_owner_id(app_state, customer_id, data).await;
    if owner_id.is_empty() {
        return Ok(());
    }

    upsert_subscription_and_status(
        app_state,
        &owner_id,
        customer_id,
        subscription_id,
        product_id,
        "active",
        previous_billing_date,
        "subscription.renewed",
        "Your subscription was renewed successfully.",
    )
    .await?;

    info!(
        "Subscription {} renewed for owner {}",
        subscription_id, owner_id
    );
    Ok(())
}

async fn handle_subscription_plan_changed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let new_product_id = data["product_id"].as_str().unwrap_or("");
    let previous_billing_date = parse_previous_billing_date(data);

    let owner_id = extract_owner_id(app_state, customer_id, data).await;
    if owner_id.is_empty() {
        return Ok(());
    }

    upsert_subscription_and_status(
        app_state,
        &owner_id,
        customer_id,
        subscription_id,
        Some(new_product_id.to_string()),
        "active",
        previous_billing_date,
        "subscription.plan_changed",
        "Your subscription plan was updated.",
    )
    .await?;

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
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = parse_previous_billing_date(data);
    let status = data["status"].as_str().unwrap_or("cancelled");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;
    if owner_id.is_empty() {
        return Ok(());
    }

    upsert_subscription_and_status(
        app_state,
        &owner_id,
        customer_id,
        subscription_id,
        product_id,
        status,
        previous_billing_date,
        "subscription.cancelled",
        "Your subscription was cancelled.",
    )
    .await?;

    info!(
        "Subscription {} cancelled for owner {}",
        subscription_id, owner_id
    );
    Ok(())
}

async fn handle_subscription_on_hold(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = parse_previous_billing_date(data);
    let status = data["status"].as_str().unwrap_or("on_hold");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;
    if owner_id.is_empty() {
        return Ok(());
    }

    upsert_subscription_and_status(
        app_state,
        &owner_id,
        customer_id,
        subscription_id,
        product_id,
        status,
        previous_billing_date,
        "subscription.on_hold",
        "Your subscription is currently on hold.",
    )
    .await?;

    info!(
        "Subscription {} on hold for owner {}",
        subscription_id, owner_id
    );
    Ok(())
}

async fn handle_subscription_failed(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = parse_previous_billing_date(data);
    let status = data["status"].as_str().unwrap_or("failed");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;
    if owner_id.is_empty() {
        return Ok(());
    }

    upsert_subscription_and_status(
        app_state,
        &owner_id,
        customer_id,
        subscription_id,
        product_id,
        status,
        previous_billing_date,
        "subscription.failed",
        "Your subscription payment failed.",
    )
    .await?;

    info!(
        "Subscription {} failed for owner {}",
        subscription_id, owner_id
    );
    Ok(())
}

async fn handle_subscription_expired(
    app_state: &AppState,
    data: &serde_json::Value,
) -> Result<(), StatusCode> {
    let customer_id = get_customer_id(data);
    let subscription_id = data["subscription_id"].as_str().unwrap_or("");
    let product_id = data["product_id"].as_str().map(|s| s.to_string());
    let previous_billing_date = parse_previous_billing_date(data);
    let status = data["status"].as_str().unwrap_or("expired");

    let owner_id = extract_owner_id(app_state, customer_id, data).await;
    if owner_id.is_empty() {
        return Ok(());
    }

    upsert_subscription_and_status(
        app_state,
        &owner_id,
        customer_id,
        subscription_id,
        product_id,
        status,
        previous_billing_date,
        "subscription.expired",
        "Your subscription has expired.",
    )
    .await?;

    info!(
        "Subscription {} expired for owner {}",
        subscription_id, owner_id
    );
    Ok(())
}
