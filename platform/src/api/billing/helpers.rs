use axum::http::StatusCode;
use tracing::error;

use crate::application::response::ApiErrorResponse;
use commands::{Command, billing::MarkCheckoutSessionCreatedCommand};
use common::{
    dodo::{CheckoutSession, CreateCheckoutParams, DodoClient},
    state::AppState,
};
use models::billing::BillingAccountWithSubscription;
use queries::{
    Query as QueryTrait,
    billing::{DodoProduct, GetBillingAccountQuery, GetDodoProductQuery},
};

use super::types::CheckoutResponse;

pub(super) async fn get_billing_account_or_404(
    state: &AppState,
    owner_id: &str,
) -> Result<BillingAccountWithSubscription, ApiErrorResponse> {
    GetBillingAccountQuery::new(owner_id.to_string())
        .execute(state)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "Billing account not found").into())
}

pub(super) async fn get_plan_product_or_404(
    state: &AppState,
    plan_name: &str,
) -> Result<DodoProduct, ApiErrorResponse> {
    GetDodoProductQuery::new(plan_name)
        .execute(state)
        .await?
        .ok_or_else(|| {
            error!("Product not found for plan: {}", plan_name);
            (StatusCode::NOT_FOUND, "Plan not found").into()
        })
}

pub(super) async fn get_pulse_product_or_500(
    state: &AppState,
) -> Result<DodoProduct, ApiErrorResponse> {
    GetDodoProductQuery::new("pulse_credits")
        .execute(state)
        .await?
        .ok_or_else(|| {
            error!("Product 'pulse_credits' not found");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Pulse product configuration missing",
            )
                .into()
        })
}

pub(super) fn create_dodo_client() -> Result<DodoClient, ApiErrorResponse> {
    DodoClient::new().map_err(|e| {
        error!("Failed to create Dodo client: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Payment gateway initialization failed",
        )
            .into()
    })
}

pub(super) async fn create_checkout_session(
    dodo: &DodoClient,
    params: CreateCheckoutParams,
    log_context: &str,
    user_message: &'static str,
) -> Result<CheckoutSession, ApiErrorResponse> {
    dodo.create_checkout_session(params).await.map_err(|e| {
        error!("Failed to create {} checkout session: {}", log_context, e);
        (StatusCode::INTERNAL_SERVER_ERROR, user_message).into()
    })
}

pub(super) async fn mark_checkout_session_created(
    state: &AppState,
    owner_id: &str,
    checkout_session_id: &str,
) -> Result<(), ApiErrorResponse> {
    MarkCheckoutSessionCreatedCommand {
        owner_id: owner_id.to_string(),
        checkout_session_id: checkout_session_id.to_string(),
    }
    .execute(state)
    .await?;
    Ok(())
}

pub(super) fn checkout_response(checkout: CheckoutSession) -> CheckoutResponse {
    CheckoutResponse {
        requires_checkout: true,
        checkout_id: Some(checkout.checkout_id),
        checkout_url: Some(checkout.checkout_url),
    }
}
