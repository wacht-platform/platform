use axum::{extract::State, response::Json};

use crate::application::{billing as billing_app, response::ApiResult};

use common::state::AppState;
use wacht::middleware::RequireAuth;

use super::types::{PortalResponse, UpdateBillingAccountRequest, owner_id_from_auth};

pub async fn get_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<Option<models::billing::BillingAccountWithSubscription>> {
    let owner_id = owner_id_from_auth(&auth);
    let account = billing_app::get_billing_account(&state, &owner_id).await?;

    Ok(account.into())
}

pub async fn update_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<UpdateBillingAccountRequest>,
) -> ApiResult<()> {
    let owner_id = owner_id_from_auth(&auth);

    billing_app::update_billing_account(
        &state,
        &owner_id,
        billing_app::UpdateBillingAccountInput {
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
        },
    )
    .await?;

    Ok(().into())
}

pub async fn get_portal_url(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PortalResponse> {
    let owner_id = owner_id_from_auth(&auth);
    let portal_url = billing_app::get_portal_url(&state, &owner_id).await?;

    Ok(PortalResponse { portal_url }.into())
}

pub async fn list_invoices(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<serde_json::Value> {
    let owner_id = owner_id_from_auth(&auth);
    let invoices = billing_app::list_invoices(&state, &owner_id).await?;

    Ok(invoices.into())
}
