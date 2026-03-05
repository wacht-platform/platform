use crate::{
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

    let invitations = DeploymentInvitationQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(params.offset.unwrap_or(0))
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(&app_state)
        .await
        .unwrap();

    let has_more = invitations.len() > limit as usize;
    let invitations = if has_more {
        invitations[..limit as usize].to_vec()
    } else {
        invitations
    };

    Ok(PaginatedResponse {
        data: invitations,
        has_more,
        limit: Some(limit),
        offset: Some(params.offset.unwrap_or(0) as i32),
    }
    .into())
}

pub async fn get_user_waitlist(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(params): QueryParams<InvitationsWaitlistQueryParams>,
) -> ApiResult<PaginatedResponse<DeploymentWaitlistUser>> {
    let limit = params.limit.unwrap_or(10) as i32;

    let waitlist = DeploymentWaitlistQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(params.offset.unwrap_or(0))
        .sort_key(params.sort_key.as_ref().map(ToString::to_string))
        .sort_order(params.sort_order.as_ref().map(ToString::to_string))
        .search(params.search.clone())
        .execute(&app_state)
        .await
        .unwrap();

    let has_more = waitlist.len() > limit as usize;
    let waitlist = if has_more {
        waitlist[..limit as usize].to_vec()
    } else {
        waitlist
    };

    Ok(PaginatedResponse {
        data: waitlist,
        has_more,
        limit: Some(limit),
        offset: Some(params.offset.unwrap_or(0) as i32),
    }
    .into())
}


pub async fn invite_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<InviteUserRequest>,
) -> ApiResult<DeploymentInvitation> {
    InviteUserCommand::new(deployment_id, request)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_invitation(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<InvitationParams>,
) -> ApiResult<()> {
    DeleteInvitationCommand::new(deployment_id, params.invitation_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn approve_waitlist_user(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WaitlistUserParams>,
) -> ApiResult<DeploymentInvitation> {
    ApproveWaitlistUserCommand::new(deployment_id, params.waitlist_user_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

