use axum::{extract::State, response::Json};

use crate::application::{billing as billing_use_cases, response::ApiResult};
use common::state::AppState;
use wacht::middleware::RequireAuth;

use super::types::{
    ChangePlanRequest, CheckoutResponse, CreateCheckoutRequest, CreatePulseCheckoutRequest,
    owner_id_from_auth,
};

pub async fn create_checkout(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreateCheckoutRequest>,
) -> ApiResult<CheckoutResponse> {
    let owner_id = owner_id_from_auth(&auth);
    let result = billing_use_cases::create_checkout(
        &state,
        &owner_id,
        billing_use_cases::CreateCheckoutInput {
            plan_name: req.plan_name,
            legal_name: req.legal_name,
            billing_email: req.billing_email,
            billing_phone: req.billing_phone,
            tax_id: req.tax_id,
            return_url: req.return_url,
        },
    )
    .await?;

    Ok(CheckoutResponse {
        requires_checkout: result.requires_checkout,
        checkout_id: result.checkout_id,
        checkout_url: result.checkout_url,
    }
    .into())
}

pub async fn change_plan(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<ChangePlanRequest>,
) -> ApiResult<CheckoutResponse> {
    let owner_id = owner_id_from_auth(&auth);
    let result = billing_use_cases::change_plan(
        &state,
        &owner_id,
        billing_use_cases::ChangePlanInput {
            plan_name: req.plan_name,
            proration_mode: req.proration_mode,
            return_url: req.return_url,
        },
    )
    .await?;

    Ok(CheckoutResponse {
        requires_checkout: result.requires_checkout,
        checkout_id: result.checkout_id,
        checkout_url: result.checkout_url,
    }
    .into())
}

pub async fn create_pulse_checkout(
    State(state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Json(req): Json<CreatePulseCheckoutRequest>,
) -> ApiResult<CheckoutResponse> {
    let owner_id = owner_id_from_auth(&auth);
    let result = billing_use_cases::create_pulse_checkout(
        &state,
        &owner_id,
        billing_use_cases::CreatePulseCheckoutInput {
            pulse_amount: req.pulse_amount,
            return_url: req.return_url,
        },
    )
    .await?;

    Ok(CheckoutResponse {
        requires_checkout: result.requires_checkout,
        checkout_id: result.checkout_id,
        checkout_url: result.checkout_url,
    }
    .into())
}
