use axum::{
    Json,
    extract::{Path, Query, State},
};

use crate::application::{
    b2b_invitation::{self, CreateOrganizationInvitationInput, OrganizationInvitationSummary},
    response::{ApiResult, PaginatedResponse},
};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::b2b::CreateOrganizationInvitationRequest;
use models::OrganizationInvitation;

use super::{
    OrganizationInvitationListQueryParams, OrganizationInvitationParams, OrganizationParams,
};

pub async fn list_organization_invitations(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Query(query): Query<OrganizationInvitationListQueryParams>,
) -> ApiResult<PaginatedResponse<OrganizationInvitation>> {
    let invitations = b2b_invitation::list_organization_invitations(
        &app_state,
        deployment_id,
        params.organization_id,
        query.workspace_id,
        query.include_deleted,
    )
    .await?;
    Ok(PaginatedResponse::from(invitations).into())
}

pub async fn create_organization_invitation(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(req): Json<CreateOrganizationInvitationRequest>,
) -> ApiResult<OrganizationInvitationSummary> {
    let summary = b2b_invitation::create_organization_invitation(
        &app_state,
        deployment_id,
        params.organization_id,
        CreateOrganizationInvitationInput {
            email: req.email,
            initial_organization_role_id: req.role_id,
            workspace_id: req.workspace_id,
            initial_workspace_role_id: req.workspace_role_id,
            expiry_days: req.expiry_days,
        },
    )
    .await?;
    Ok(summary.into())
}

pub async fn discard_organization_invitation(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationInvitationParams>,
) -> ApiResult<()> {
    b2b_invitation::discard_organization_invitation(
        &app_state,
        deployment_id,
        params.organization_id,
        params.invitation_id,
    )
    .await?;
    Ok(().into())
}
