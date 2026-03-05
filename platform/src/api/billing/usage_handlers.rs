use axum::{extract::State, http::StatusCode};

use crate::application::response::{ApiResult, PaginatedResponse};

use common::state::AppState;
use queries::{
    Query as QueryTrait,
    billing::{GetBillingAccountQuery, GetBillingAccountUsageQuery},
};
use wacht::middleware::RequireAuth;

use super::types::{UsageResponse, owner_id_from_auth};

pub async fn get_current_usage(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<UsageResponse> {
    let account_with_sub = GetBillingAccountQuery::new(owner_id_from_auth(&auth))
        .execute(&state)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "Billing account not found"))?;

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
    let account = GetBillingAccountQuery::new(owner_id_from_auth(&auth))
        .execute(&state)
        .await?
        .ok_or((StatusCode::NOT_FOUND, "Billing account not found"))?;

    let transactions =
        queries::billing::ListPulseTransactionsQuery::new(account.billing_account.id)
            .execute(&state)
            .await?;

    Ok(PaginatedResponse::from(transactions).into())
}
