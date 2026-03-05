use crate::{
    api::pagination::paginate_results,
    application::response::{ApiResult, PaginatedResponse},
    middleware::RequireDeployment,
};
use common::state::AppState;

use commands::{ApproveWaitlistUserCommand, Command, DeleteInvitationCommand, InviteUserCommand};
use dto::{json::InviteUserRequest, query::InvitationsWaitlistQueryParams};
use models::{DeploymentInvitation, DeploymentWaitlistUser};
use queries::{DeploymentInvitationQuery, DeploymentWaitlistQuery, Query};

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
    let limit = params.limit.unwrap_or(10) as i32;
    let offset = params.offset.unwrap_or(0);

    let invitations = DeploymentInvitationQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(&app_state)
        .await?;

    Ok(paginate_results(invitations, limit, Some(offset)).into())
}

pub async fn get_user_waitlist(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<InvitationsWaitlistQueryParams>,
) -> ApiResult<PaginatedResponse<DeploymentWaitlistUser>> {
    let limit = params.limit.unwrap_or(10) as i32;
    let offset = params.offset.unwrap_or(0);

    let waitlist = DeploymentWaitlistQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(&app_state)
        .await?;

    Ok(paginate_results(waitlist, limit, Some(offset)).into())
}

pub async fn invite_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<InviteUserRequest>,
) -> ApiResult<DeploymentInvitation> {
    let invitation = InviteUserCommand::new(deployment_id, request)
        .execute(&app_state)
        .await?;
    Ok(invitation.into())
}

pub async fn delete_invitation(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<InvitationParams>,
) -> ApiResult<()> {
    DeleteInvitationCommand::new(deployment_id, params.invitation_id)
        .execute(&app_state)
        .await?;
    Ok(().into())
}

pub async fn approve_waitlist_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WaitlistUserParams>,
) -> ApiResult<DeploymentInvitation> {
    let invitation = ApproveWaitlistUserCommand::new(deployment_id, params.waitlist_user_id)
        .execute(&app_state)
        .await?;
    Ok(invitation.into())
}
