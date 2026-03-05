use axum::{extract::State, http::StatusCode};

use crate::application::response::{ApiResult, PaginatedResponse};

use common::state::AppState;
use queries::{Query as QueryTrait, billing::GetBillingAccountUsageQuery};
use wacht::middleware::RequireAuth;

use super::helpers::get_billing_account_or_404;
use super::types::{UsageResponse, owner_id_from_auth};

pub async fn get_current_usage(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<UsageResponse> {
    let owner_id = owner_id_from_auth(&auth);
    let account_with_sub = get_billing_account_or_404(&state, &owner_id).await?;

    let subscription = account_with_sub.subscription.ok_or((
        StatusCode::NOT_FOUND,
        "No active subscription found for this billing account",
    ))?;

    let billing_period_timestamp = subscription.previous_billing_date.ok_or((
        StatusCode::INTERNAL_SERVER_ERROR,
        "Subscription missing previous_billing_date",
    ))?;

    let snapshots = GetBillingAccountUsageQuery::new(
        account_with_sub.billing_account.id,
        billing_period_timestamp,
    )
    .execute(&state)
    .await?;

    Ok(UsageResponse {
        snapshots,
        billing_period: billing_period_timestamp.to_rfc3339(),
    }
    .into())
}

pub async fn list_pulse_transactions(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PaginatedResponse<models::pulse_transaction::PulseTransaction>> {
    let owner_id = owner_id_from_auth(&auth);
    let account = get_billing_account_or_404(&state, &owner_id).await?;

    let transactions =
        queries::billing::ListPulseTransactionsQuery::new(account.billing_account.id)
            .execute(&state)
            .await?;

    Ok(PaginatedResponse::from(transactions).into())
}
