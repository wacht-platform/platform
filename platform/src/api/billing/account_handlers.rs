use axum::extract::State;

use crate::application::{billing as billing_app, response::ApiResult};

use common::state::AppState;
use wacht::middleware::RequireAuth;

use super::types::{PortalResponse, owner_id_from_auth};

pub async fn get_billing_account(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<Option<models::billing::BillingAccountWithSubscription>> {
    let owner_id = owner_id_from_auth(&auth);
    let account = billing_app::get_billing_account(&state, &owner_id).await?;

    Ok(account.into())
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
