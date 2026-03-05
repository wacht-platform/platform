use axum::{extract::State, http::StatusCode, response::Json};

use crate::application::response::ApiResult;

use commands::{Command, billing::UpdateBillingAccountCommand};
use common::dodo::DodoClient;
use common::state::AppState;
use queries::{Query as QueryTrait, billing::GetBillingAccountQuery};
use tracing::error;
use wacht::middleware::RequireAuth;

use super::types::{
    PortalResponse, UpdateBillingAccountRequest, enforce_checkout_cooldown, owner_id_from_auth,
};

pub async fn get_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<Option<models::billing::BillingAccountWithSubscription>> {
    let account = GetBillingAccountQuery::new(owner_id_from_auth(&auth))
        .execute(&state)
        .await?;

    Ok(account.into())
}

pub async fn update_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<UpdateBillingAccountRequest>,
) -> ApiResult<()> {
    let owner_id = owner_id_from_auth(&auth);

    let existing = GetBillingAccountQuery::new(owner_id)
        .execute(&state)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "Billing account not found"))?;

    if let Err(err) = enforce_checkout_cooldown(&existing) {
        return Err(err.into());
    }

    UpdateBillingAccountCommand {
        id: existing.billing_account.id,
        legal_name: req.legal_name,
        billing_email: req.billing_email,
        billing_phone: req.billing_phone,
        tax_id: req.tax_id,
        address_line1: req.address_line1,
        address_line2: req.address_line2,
        city: req.city,
        state: req.state,
        postal_code: req.postal_code,
        country: req.country,
    }
    .execute(&state)
    .await?;

    Ok(().into())
}

pub async fn get_portal_url(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PortalResponse> {
    let account = GetBillingAccountQuery::new(owner_id_from_auth(&auth))
        .execute(&state)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "Billing account not found"))?;

    let provider_customer_id = account
        .billing_account
        .provider_customer_id
        .ok_or((StatusCode::NOT_FOUND, "Payment provider customer not found"))?;

    let dodo = DodoClient::new().map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let portal = dodo
        .create_portal_session(&provider_customer_id)
        .await
        .map_err(|e| {
            error!("Failed to create portal session: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to create portal session",
            )
        })?;

    Ok(PortalResponse {
        portal_url: portal.url,
    }
    .into())
}

pub async fn list_invoices(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<serde_json::Value> {
    let account = GetBillingAccountQuery::new(owner_id_from_auth(&auth))
        .execute(&state)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "Billing account not found"))?;

    let invoices = queries::billing::ListBillingInvoicesQuery::new(account.billing_account.id)
        .execute(&state)
        .await?;

    Ok(serde_json::json!({ "items": invoices }).into())
}
