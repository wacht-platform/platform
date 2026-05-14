use crate::{
    application::{
        response::{ApiResult, PaginatedResponse},
        user_core as user_core_app,
    },
    middleware::RequireDeployment,
};
use common::state::AppState;
use models::UserPasskey;
use serde::Deserialize;

use axum::{
    Json,
    extract::{Path, State},
};

use super::types::{UserParams, UserPasskeyParams};

#[derive(Deserialize)]
pub struct RenamePasskeyRequest {
    pub name: String,
}

pub async fn get_user_passkeys(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<PaginatedResponse<UserPasskey>> {
    let passkeys =
        user_core_app::get_user_passkeys(&app_state, deployment_id, params.user_id).await?;
    Ok(PaginatedResponse::from(passkeys).into())
}

pub async fn rename_user_passkey(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserPasskeyParams>,
    Json(request): Json<RenamePasskeyRequest>,
) -> ApiResult<()> {
    user_core_app::rename_user_passkey(
        &app_state,
        deployment_id,
        params.user_id,
        params.passkey_id,
        request.name,
    )
    .await?;
    Ok(().into())
}

pub async fn delete_user_passkey(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserPasskeyParams>,
) -> ApiResult<()> {
    user_core_app::delete_user_passkey(
        &app_state,
        deployment_id,
        params.user_id,
        params.passkey_id,
    )
    .await?;
    Ok(().into())
}
