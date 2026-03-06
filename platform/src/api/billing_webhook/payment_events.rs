use super::notifications::{extract_owner_id, send_billing_change_email};
use axum::http::StatusCode;
use commands::{
    billing::{
        MarkCheckoutFlowFailedCommand, MarkPaymentSucceededCommand,
        UpdateBillingAccountStatusCommand, UpsertInvoiceCommand,
    },
    pulse::AddPulseCreditsCommand,
};
use common::{db_router::ReadConsistency, state::AppState};
use models::pulse_transaction::PulseTransactionType;
use queries::billing::GetBillingAccountQuery;
use tracing::{error, info, warn};

use super::get_customer_id;

pub(super) async fn handle_payment_succeeded(
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
            let status = match GetBillingAccountQuery::new(owner_id.clone())
                .execute_with(app_state.db_router.reader(ReadConsistency::Strong))
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

            UpdateBillingAccountStatusCommand {
                owner_id: owner_id.clone(),
                status: status.to_string(),
            }
            .execute_with(app_state.db_router.writer())
            .await
            .map_err(|e| {
                error!(
                    "Failed to update billing account status on payment success: {}",
                    e
                );
                StatusCode::INTERNAL_SERVER_ERROR
            })?;

            MarkPaymentSucceededCommand {
                owner_id: owner_id.clone(),
                webhook_event: "payment.succeeded".to_string(),
            }
            .execute_with(app_state.db_router.writer())
            .await
            .map_err(|e| {
                error!("Failed to update checkout flow state: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })?;
        }

        UpsertInvoiceCommand {
            owner_id: owner_id.clone(),
            provider_payment_id: payment_id.to_string(),
            provider_customer_id: customer_id.to_string(),
            amount_due_cents: amount,
            amount_paid_cents: amount,
            currency: currency.to_string(),
            status: "paid".to_string(),
            invoice_pdf_url: Some(format!(
                "https://live.dodopayments.com/invoices/payments/{}",
                payment_id
            )),
            hosted_invoice_url: None,
            invoice_number: None,
            due_date: None,
            paid_at: Some(chrono::Utc::now()),
            period_start: None,
            period_end: None,
            metadata: serde_json::json!({}),
        }
        .execute_with(
            app_state.db_router.writer(),
            app_state.sf.next_id().map_err(|e| {
                error!("Failed to generate billing invoice id: {}", e);
                StatusCode::INTERNAL_SERVER_ERROR
            })? as i64,
        )
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
                    let add_pulse_command = AddPulseCreditsCommand {
                        owner_id: owner_id.clone(),
                        amount_pulse_cents: pulse_to_add,
                        transaction_type: PulseTransactionType::Purchase,
                        reference_id: Some(payment_id.to_string()),
                    };
                    add_pulse_command
                        .execute_with(
                            app_state.db_router.writer(),
                            app_state.sf.next_id().map_err(|e| {
                                error!("Failed to generate pulse transaction id: {}", e);
                                StatusCode::INTERNAL_SERVER_ERROR
                            })? as i64,
                        )
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

pub(super) async fn handle_payment_failed(
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
        .execute_with(app_state.db_router.writer())
        .await
        .map_err(|e| {
            error!(
                "Failed to update billing account status on payment failure: {}",
                e
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

        MarkCheckoutFlowFailedCommand {
            owner_id: owner_id.clone(),
            webhook_event: "payment.failed".to_string(),
            reason: "payment_failed".to_string(),
        }
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
