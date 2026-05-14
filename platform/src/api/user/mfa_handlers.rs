use crate::{
    application::{response::ApiResult, user_core as user_core_app},
    middleware::RequireDeployment,
};
use common::state::AppState;
use serde::{Deserialize, Serialize};

use axum::{
    Json,
    extract::{Path, State},
};

use super::types::UserParams;

#[derive(Serialize)]
pub struct BackupCodesResponse {
    pub backup_codes: Vec<String>,
}

#[derive(Deserialize)]
pub struct CreateAuthenticatorRequest {
    /// Base32-encoded TOTP secret. Whitespace and `-` separators are stripped
    /// before validation; the secret must decode to at least 16 bytes (128
    /// bits).
    pub secret: String,
    /// Optional label shown in the user's authenticator app. Defaults to the
    /// user's primary email / username / name if omitted.
    #[serde(default)]
    pub account_name: Option<String>,
}

#[derive(Serialize)]
pub struct CreateAuthenticatorResponse {
    #[serde(with = "models::utils::serde::i64_as_string")]
    pub id: i64,
    /// otpauth:// URL the admin can render as a QR code for the user. The
    /// secret appears in the URL's query string per the otpauth spec.
    pub otp_url: String,
}

pub async fn create_user_authenticator(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Json(req): Json<CreateAuthenticatorRequest>,
) -> ApiResult<CreateAuthenticatorResponse> {
    let resp = user_core_app::create_user_authenticator(
        &app_state,
        deployment_id,
        params.user_id,
        req.secret,
        req.account_name,
    )
    .await?;
    Ok(CreateAuthenticatorResponse {
        id: resp.id,
        otp_url: resp.otp_url,
    }
    .into())
}

pub async fn delete_user_authenticator(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<()> {
    user_core_app::delete_user_authenticator(&app_state, deployment_id, params.user_id).await?;
    Ok(().into())
}

pub async fn regenerate_user_backup_codes(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<BackupCodesResponse> {
    let backup_codes =
        user_core_app::regenerate_user_backup_codes(&app_state, deployment_id, params.user_id)
            .await?;
    Ok(BackupCodesResponse { backup_codes }.into())
}
