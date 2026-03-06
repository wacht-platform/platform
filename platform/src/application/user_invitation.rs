use commands::{ApproveWaitlistUserCommand, Command, DeleteInvitationCommand, InviteUserCommand};
use common::error::AppError;
use common::state::AppState;
use dto::{json::InviteUserRequest, query::InvitationsWaitlistQueryParams};
use models::{DeploymentInvitation, DeploymentWaitlistUser};
use queries::{DeploymentInvitationQuery, DeploymentWaitlistQuery, Query};

use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};

pub async fn get_invited_user_list(
    app_state: &AppState,
    deployment_id: i64,
    params: InvitationsWaitlistQueryParams,
) -> Result<PaginatedResponse<DeploymentInvitation>, AppError> {
    let limit = params.limit.unwrap_or(10) as i32;
    let offset = params.offset.unwrap_or(0);

    let invitations = DeploymentInvitationQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(app_state)
        .await?;

    Ok(paginate_results(invitations, limit, Some(offset)))
}

pub async fn get_user_waitlist(
    app_state: &AppState,
    deployment_id: i64,
    params: InvitationsWaitlistQueryParams,
) -> Result<PaginatedResponse<DeploymentWaitlistUser>, AppError> {
    let limit = params.limit.unwrap_or(10) as i32;
    let offset = params.offset.unwrap_or(0);

    let waitlist = DeploymentWaitlistQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(app_state)
        .await?;

    Ok(paginate_results(waitlist, limit, Some(offset)))
}

pub async fn invite_user(
    app_state: &AppState,
    deployment_id: i64,
    request: InviteUserRequest,
) -> Result<DeploymentInvitation, AppError> {
    InviteUserCommand::new(deployment_id, request)
        .execute(app_state)
        .await
}

pub async fn delete_invitation(
    app_state: &AppState,
    deployment_id: i64,
    invitation_id: i64,
) -> Result<(), AppError> {
    DeleteInvitationCommand::new(deployment_id, invitation_id)
        .execute(app_state)
        .await?;
    Ok(())
}

pub async fn approve_waitlist_user(
    app_state: &AppState,
    deployment_id: i64,
    waitlist_user_id: i64,
) -> Result<DeploymentInvitation, AppError> {
    ApproveWaitlistUserCommand::new(deployment_id, waitlist_user_id)
        .execute(app_state)
        .await
}
