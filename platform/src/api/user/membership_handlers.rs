use crate::{
    application::{
        response::{ApiResult, PaginatedResponse},
        user_core as user_core_app,
    },
    middleware::RequireDeployment,
};
use common::state::AppState;
use models::{UserOrganizationMembership, UserWorkspaceMembership};

use axum::extract::{Path, State};

use super::types::UserParams;

pub async fn get_user_organization_memberships(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<PaginatedResponse<UserOrganizationMembership>> {
    let memberships =
        user_core_app::get_user_organization_memberships(&app_state, deployment_id, params.user_id)
            .await?;
    Ok(PaginatedResponse::from(memberships).into())
}

pub async fn get_user_workspace_memberships(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UserParams>,
) -> ApiResult<PaginatedResponse<UserWorkspaceMembership>> {
    let memberships =
        user_core_app::get_user_workspace_memberships(&app_state, deployment_id, params.user_id)
            .await?;
    Ok(PaginatedResponse::from(memberships).into())
}
