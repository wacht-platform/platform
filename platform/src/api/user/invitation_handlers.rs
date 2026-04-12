use crate::{
    application::{
        response::{ApiResult, PaginatedResponse},
        user_invitation as user_invitation_app,
    },
    middleware::RequireDeployment,
};
use common::state::AppState;

use dto::{json::InviteUserRequest, query::InvitationsWaitlistQueryParams};
use models::{DeploymentInvitation, DeploymentWaitlistUser};

use axum::{
    Json,
    extract::{Path, Query as QueryParams, State},
};

use super::types::{InvitationParams, WaitlistUserParams};

pub async fn get_invited_user_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<InvitationsWaitlistQueryParams>,
) -> ApiResult<PaginatedResponse<DeploymentInvitation>> {
    let invitations =
        user_invitation_app::get_invited_user_list(&app_state, deployment_id, params).await?;

    Ok(invitations.into())
}

pub async fn get_user_waitlist(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<InvitationsWaitlistQueryParams>,
) -> ApiResult<PaginatedResponse<DeploymentWaitlistUser>> {
    let waitlist =
        user_invitation_app::get_user_waitlist(&app_state, deployment_id, params).await?;

    Ok(waitlist.into())
}

pub async fn invite_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<InviteUserRequest>,
) -> ApiResult<DeploymentInvitation> {
    let invitation = user_invitation_app::invite_user(&app_state, deployment_id, request).await?;
    Ok(invitation.into())
}

pub async fn delete_invitation(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<InvitationParams>,
) -> ApiResult<()> {
    user_invitation_app::delete_invitation(&app_state, deployment_id, params.invitation_id).await?;
    Ok(().into())
}

pub async fn approve_waitlist_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WaitlistUserParams>,
) -> ApiResult<DeploymentInvitation> {
    let invitation = user_invitation_app::approve_waitlist_user(
        &app_state,
        deployment_id,
        params.waitlist_user_id,
    )
    .await?;
    Ok(invitation.into())
}
