use crate::{
    application::{
        response::{ApiResult, PaginatedResponse},
        user_core as user_core_app,
    },
    middleware::RequireDeployment,
};
use common::state::AppState;
use models::SignIn;
use serde::{Deserialize, Serialize};

use axum::extract::{Path, Query, State};

use super::types::{UserParams, UserSigninParams};

#[derive(Deserialize, Default)]
pub struct ListSigninsQuery {
    #[serde(default)]
    pub include_expired: bool,
}

#[derive(Serialize)]
pub struct RevokeAllResponse {
    pub revoked: u64,
}

pub async fn get_user_signins(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
    Query(query): Query<ListSigninsQuery>,
) -> ApiResult<PaginatedResponse<SignIn>> {
    let signins = user_core_app::get_user_signins(
        &app_state,
        deployment_id,
        params.user_id,
        query.include_expired,
    )
    .await?;
    Ok(PaginatedResponse::from(signins).into())
}

pub async fn revoke_user_signin(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserSigninParams>,
) -> ApiResult<()> {
    user_core_app::revoke_user_signin(&app_state, deployment_id, params.user_id, params.signin_id)
        .await?;
    Ok(().into())
}

pub async fn revoke_all_user_signins(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<RevokeAllResponse> {
    let revoked =
        user_core_app::revoke_all_user_signins(&app_state, deployment_id, params.user_id).await?;
    Ok(RevokeAllResponse { revoked }.into())
}
