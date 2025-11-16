use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
};
use commands::{
    Command,
    billing::{UpdateBillingAccountStatusCommand, UpsertSubscriptionCommand, UpsertInvoiceCommand, UpdateBillingAccountFromWebhookCommand},
};
use common::chargebee::{ChargebeeClient, UpdateSubscriptionParams};
use common::state::AppState;
use tracing::{error, info};

// Webhook endpoint for Chargebee events
pub async fn handle_chargebee_webhook(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Result<StatusCode, StatusCode> {
    // Verify webhook signature
    let signature = headers
        .get("X-Chargebee-Signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let chargebee = ChargebeeClient::new().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !chargebee.verify_webhook_signature(&body, signature) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Parse webhook event
    let event: serde_json::Value =
        serde_json::from_str(&body).map_err(|_| StatusCode::BAD_REQUEST)?;

    let event_type = event["event_type"].as_str().unwrap_or("");

    // Handle subscription events
    match event_type {
        "customer_created" | "customer_changed" => {
            if let Some(customer) = event["content"]["customer"].as_object() {
                let customer_id = customer["id"].as_str().unwrap_or("");

                if customer_id.starts_with("user_") || customer_id.starts_with("org_") {
                    // Extract customer details
                    let first_name = customer["first_name"].as_str().map(|s| s.to_string());
                    let last_name = customer["last_name"].as_str().map(|s| s.to_string());
                    let company = customer["company"].as_str().map(|s| s.to_string());
                    let email = customer["email"].as_str().map(|s| s.to_string());
                    let phone = customer["phone"].as_str().map(|s| s.to_string());

                    // Construct full name from first and last name
                    let legal_name = match (first_name, last_name) {
                        (Some(first), Some(last)) => Some(format!("{} {}", first, last)),
                        (Some(first), None) => Some(first),
                        (None, Some(last)) => Some(last),
                        (None, None) => company.clone(),
                    };

                    // Extract billing address
                    let billing_address = customer["billing_address"].as_object();
                    let address_line1 = billing_address
                        .and_then(|addr| addr["line1"].as_str())
                        .map(|s| s.to_string());
                    let address_line2 = billing_address
                        .and_then(|addr| addr["line2"].as_str())
                        .map(|s| s.to_string());
                    let city = billing_address
                        .and_then(|addr| addr["city"].as_str())
                        .map(|s| s.to_string());
                    let state = billing_address
                        .and_then(|addr| addr["state"].as_str())
                        .map(|s| s.to_string());
                    let postal_code = billing_address
                        .and_then(|addr| addr["zip"].as_str())
                        .map(|s| s.to_string());
                    let country = billing_address
                        .and_then(|addr| addr["country"].as_str())
                        .map(|s| s.to_string());

                    // Update billing account with customer data from Chargebee
                    UpdateBillingAccountFromWebhookCommand {
                        owner_id: customer_id.to_string(),
                        legal_name,
                        billing_email: email,
                        billing_phone: phone,
                        company,
                        address_line1,
                        address_line2,
                        city,
                        state,
                        postal_code,
                        country,
                    }
                    .execute(&app_state)
                    .await
                    .map_err(|e| {
                        error!("Failed to update billing account from webhook: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR
                    })?;

                    info!("Updated billing account {} from Chargebee customer data", customer_id);
                }
            }
        }
        "subscription_created" => {
            if let Some(subscription) = event["content"]["subscription"].as_object() {
                if let Some(customer) = event["content"]["customer"].as_object() {
                    let customer_id = customer["id"].as_str().unwrap_or("");
                    let subscription_id = subscription["id"].as_str().unwrap_or("");

                    // Use customer_id directly as owner_id (format: "user_123" or "org_123")
                    if customer_id.starts_with("user_") || customer_id.starts_with("org_") {
                        let status = subscription["status"].as_str().unwrap_or("active");

                        // Save subscription to database
                        UpsertSubscriptionCommand {
                            owner_id: customer_id.to_string(),
                            chargebee_customer_id: customer_id.to_string(),
                            chargebee_subscription_id: subscription_id.to_string(),
                            status: status.to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|e| {
                            error!("Failed to upsert subscription: {}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                        // Update billing account status to active
                        UpdateBillingAccountStatusCommand {
                            owner_id: customer_id.to_string(),
                            status: "active".to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|e| {
                            error!("Failed to update billing account status: {}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                        // Set threshold billing for the subscription
                        // $50 threshold for Growth plan, $500 for Enterprise
                        let plan_id = subscription["plan_id"].as_str().unwrap_or("");
                        let threshold = match plan_id {
                            "growth_monthly" => Some(5000),     // $50 in cents
                            "enterprise_custom" => Some(50000), // $500 in cents
                            _ => None,
                        };

                        if let Some(threshold_amount) = threshold {
                            info!(
                                "Setting threshold billing of ${} for subscription {}",
                                threshold_amount / 100,
                                subscription_id
                            );

                            if let Err(e) = chargebee
                                .update_subscription(
                                    subscription_id,
                                    UpdateSubscriptionParams {
                                        plan_id: None,
                                        plan_quantity: None,
                                        trial_end: None,
                                        invoice_immediately: Some(true),
                                        invoice_immediately_min_amount: Some(threshold_amount),
                                    },
                                )
                                .await
                            {
                                error!("Failed to set threshold billing: {}", e);
                                // Don't fail the webhook, just log the error
                            }
                        }
                    }
                }
            }
        }
        "subscription_changed" | "subscription_cancelled" | "subscription_reactivated" => {
            if let Some(subscription) = event["content"]["subscription"].as_object() {
                if let Some(customer) = event["content"]["customer"].as_object() {
                    let customer_id = customer["id"].as_str().unwrap_or("");

                    // Use customer_id directly as owner_id (format: "user_123" or "org_123")
                    if customer_id.starts_with("user_") || customer_id.starts_with("org_") {
                        let status = subscription["status"].as_str().unwrap_or("active");

                        UpsertSubscriptionCommand {
                            owner_id: customer_id.to_string(),
                            chargebee_customer_id: customer_id.to_string(),
                            chargebee_subscription_id: subscription["id"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            status: status.to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                        // Update billing account status based on subscription status
                        let account_status = match status {
                            "cancelled" | "non_renewing" => "cancelled",
                            "active" | "future" => "active",
                            _ => "active",
                        };

                        UpdateBillingAccountStatusCommand {
                            owner_id: customer_id.to_string(),
                            status: account_status.to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|e| {
                            error!("Failed to update billing account status: {}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                    }
                }
            }
        }
        "payment_failed" => {
            if let Some(invoice) = event["content"]["invoice"].as_object() {
                if let Some(customer) = event["content"]["customer"].as_object() {
                    let customer_id = customer["id"].as_str().unwrap_or("");

                    // Use customer_id directly as owner_id (format: "user_123" or "org_123")
                    if customer_id.starts_with("user_") || customer_id.starts_with("org_") {
                        // Update billing account status to failed
                        UpdateBillingAccountStatusCommand {
                            owner_id: customer_id.to_string(),
                            status: "failed".to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|e| {
                            error!(
                                "Failed to update billing account status on payment failure: {}",
                                e
                            );
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                        info!(
                            "Payment failed for customer {}, invoice {}",
                            customer_id,
                            invoice["id"].as_str().unwrap_or("")
                        );
                    }
                }
            }
        }
        "payment_succeeded" => {
            if let Some(invoice) = event["content"]["invoice"].as_object() {
                if let Some(customer) = event["content"]["customer"].as_object() {
                    let customer_id = customer["id"].as_str().unwrap_or("");

                    // Use customer_id directly as owner_id (format: "user_123" or "org_123")
                    if customer_id.starts_with("user_") || customer_id.starts_with("org_") {
                        // Update billing account status back to active after successful payment
                        UpdateBillingAccountStatusCommand {
                            owner_id: customer_id.to_string(),
                            status: "active".to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|e| {
                            error!(
                                "Failed to update billing account status on payment success: {}",
                                e
                            );
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                        info!(
                            "Payment succeeded for customer {}, invoice {}",
                            customer_id,
                            invoice["id"].as_str().unwrap_or("")
                        );
                    }
                }
            }
        }
        "invoice_generated" | "invoice_updated" => {
            if let Some(invoice) = event["content"]["invoice"].as_object() {
                if let Some(customer) = event["content"]["customer"].as_object() {
                    let customer_id = customer["id"].as_str().unwrap_or("");

                    if customer_id.starts_with("user_") || customer_id.starts_with("org_") {
                        let invoice_id = invoice["id"].as_str().unwrap_or("").to_string();
                        let amount_due = invoice["amount_due"].as_i64().unwrap_or(0);
                        let amount_paid = invoice["amount_paid"].as_i64().unwrap_or(0);
                        let currency = invoice["currency_code"].as_str().unwrap_or("USD").to_string();
                        let status = invoice["status"].as_str().unwrap_or("open").to_string();
                        let invoice_number = invoice["number"].as_str().map(|s| s.to_string());
                        let invoice_pdf_url = invoice["invoice_pdf"].as_str().map(|s| s.to_string());
                        let hosted_invoice_url = invoice["hosted_invoice_url"].as_str().map(|s| s.to_string());

                        // Parse dates
                        let due_date = invoice["due_date"].as_i64().map(|ts| {
                            chrono::DateTime::from_timestamp(ts, 0).unwrap()
                        });
                        let paid_at = invoice["paid_at"].as_i64().map(|ts| {
                            chrono::DateTime::from_timestamp(ts, 0).unwrap()
                        });
                        let period_start = invoice["line_items"].as_array()
                            .and_then(|items| items.get(0))
                            .and_then(|item| item["date_from"].as_i64())
                            .map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap());
                        let period_end = invoice["line_items"].as_array()
                            .and_then(|items| items.get(0))
                            .and_then(|item| item["date_to"].as_i64())
                            .map(|ts| chrono::DateTime::from_timestamp(ts, 0).unwrap());

                        UpsertInvoiceCommand {
                            owner_id: customer_id.to_string(),
                            chargebee_invoice_id: invoice_id,
                            chargebee_customer_id: customer_id.to_string(),
                            amount_due_cents: amount_due,
                            amount_paid_cents: amount_paid,
                            currency,
                            status,
                            invoice_pdf_url,
                            hosted_invoice_url,
                            invoice_number,
                            due_date,
                            paid_at,
                            period_start,
                            period_end,
                            metadata: serde_json::json!({}),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|e| {
                            error!("Failed to upsert invoice: {}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;

                        info!("Invoice {} saved for customer {}", invoice["id"].as_str().unwrap_or(""), customer_id);
                    }
                }
            }
        }
        "subscription_paused" | "subscription_resumed" => {
            if let Some(subscription) = event["content"]["subscription"].as_object() {
                if let Some(customer) = event["content"]["customer"].as_object() {
                    let customer_id = customer["id"].as_str().unwrap_or("");

                    // Use customer_id directly as owner_id (format: "user_123" or "org_123")
                    if customer_id.starts_with("user_") || customer_id.starts_with("org_") {
                        let status = subscription["status"].as_str().unwrap_or("active");

                        // Update subscription status
                        UpsertSubscriptionCommand {
                            owner_id: customer_id.to_string(),
                            chargebee_customer_id: customer_id.to_string(),
                            chargebee_subscription_id: subscription["id"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                            status: status.to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

                        // Update billing account status
                        let account_status = match status {
                            "paused" => "paused",
                            "active" => "active",
                            _ => status,
                        };

                        UpdateBillingAccountStatusCommand {
                            owner_id: customer_id.to_string(),
                            status: account_status.to_string(),
                        }
                        .execute(&app_state)
                        .await
                        .map_err(|e| {
                            error!("Failed to update billing account status: {}", e);
                            StatusCode::INTERNAL_SERVER_ERROR
                        })?;
                    }
                }
            }
        }
        _ => {
            // Log unhandled events for monitoring
            info!("Received unhandled webhook event: {}", event_type);
        }
    }

    Ok(StatusCode::OK)
}
