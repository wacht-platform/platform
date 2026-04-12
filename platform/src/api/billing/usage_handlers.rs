use axum::extract::State;

use crate::application::{
    billing as billing_app,
    response::{ApiResult, PaginatedResponse},
};

use common::state::AppState;
use wacht::middleware::RequireAuth;

use super::types::{UsageResponse, owner_id_from_auth};

pub async fn get_current_usage(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<UsageResponse> {
    let owner_id = owner_id_from_auth(&auth);
    let (snapshots, billing_period) = billing_app::get_current_usage(&state, &owner_id).await?;

    Ok(UsageResponse {
        snapshots,
        billing_period,
    }
    .into())
}

pub async fn list_pulse_transactions(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PaginatedResponse<models::pulse_transaction::PulseTransaction>> {
    let owner_id = owner_id_from_auth(&auth);
    let transactions = billing_app::list_pulse_transactions(&state, &owner_id).await?;

    Ok(PaginatedResponse::from(transactions).into())
}
