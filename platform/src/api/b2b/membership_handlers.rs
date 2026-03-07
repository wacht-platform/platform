use axum::{
    Json,
    extract::{Path, State},
};

use crate::application::{b2b_membership as b2b_membership_app, response::ApiResult};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::b2b::{
    AddOrganizationMemberRequest, AddWorkspaceMemberRequest, CreateOrganizationRoleRequest,
    CreateWorkspaceRoleRequest, UpdateOrganizationMemberRequest, UpdateOrganizationRoleRequest,
    UpdateWorkspaceMemberRequest, UpdateWorkspaceRoleRequest,
};
use models::{OrganizationMemberDetails, OrganizationRole, WorkspaceMemberDetails, WorkspaceRole};

use super::{
    OrganizationMemberParams, OrganizationParams, OrganizationRoleParams, WorkspaceMemberParams,
    WorkspaceParams, WorkspaceRoleParams,
};

pub async fn add_organization_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(request): Json<AddOrganizationMemberRequest>,
) -> ApiResult<OrganizationMemberDetails> {
    let member = b2b_membership_app::add_organization_member(
        &app_state,
        deployment_id,
        params.organization_id,
        request,
    )
    .await?;

    Ok(member.into())
}

pub async fn update_organization_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationMemberParams>,
    Json(request): Json<UpdateOrganizationMemberRequest>,
) -> ApiResult<()> {
    b2b_membership_app::update_organization_member(
        &app_state,
        deployment_id,
        params.organization_id,
        params.membership_id,
        request,
    )
    .await?;

    Ok(().into())
}

pub async fn remove_organization_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationMemberParams>,
) -> ApiResult<()> {
    b2b_membership_app::remove_organization_member(
        &app_state,
        deployment_id,
        params.organization_id,
        params.membership_id,
    )
    .await?;

    Ok(().into())
}

pub async fn create_organization_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(request): Json<CreateOrganizationRoleRequest>,
) -> ApiResult<OrganizationRole> {
    let role = b2b_membership_app::create_organization_role(
        &app_state,
        deployment_id,
        params.organization_id,
        request,
    )
    .await?;

    Ok(role.into())
}

pub async fn update_organization_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationRoleParams>,
    Json(request): Json<UpdateOrganizationRoleRequest>,
) -> ApiResult<OrganizationRole> {
    let role = b2b_membership_app::update_organization_role(
        &app_state,
        deployment_id,
        params.organization_id,
        params.role_id,
        request,
    )
    .await?;

    Ok(role.into())
}

pub async fn delete_organization_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationRoleParams>,
) -> ApiResult<()> {
    b2b_membership_app::delete_organization_role(
        &app_state,
        deployment_id,
        params.organization_id,
        params.role_id,
    )
    .await?;

    Ok(().into())
}

pub async fn create_workspace_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    Json(request): Json<CreateWorkspaceRoleRequest>,
) -> ApiResult<WorkspaceRole> {
    let role = b2b_membership_app::create_workspace_role(
        &app_state,
        deployment_id,
        params.workspace_id,
        request,
    )
    .await?;

    Ok(role.into())
}

pub async fn update_workspace_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceRoleParams>,
    Json(request): Json<UpdateWorkspaceRoleRequest>,
) -> ApiResult<WorkspaceRole> {
    let role = b2b_membership_app::update_workspace_role(
        &app_state,
        deployment_id,
        params.workspace_id,
        params.role_id,
        request,
    )
    .await?;

    Ok(role.into())
}

pub async fn delete_workspace_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceRoleParams>,
) -> ApiResult<()> {
    b2b_membership_app::delete_workspace_role(
        &app_state,
        deployment_id,
        params.workspace_id,
        params.role_id,
    )
    .await?;

    Ok(().into())
}

pub async fn add_workspace_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    Json(request): Json<AddWorkspaceMemberRequest>,
) -> ApiResult<WorkspaceMemberDetails> {
    let member = b2b_membership_app::add_workspace_member(
        &app_state,
        deployment_id,
        params.workspace_id,
        request,
    )
    .await?;

    Ok(member.into())
}

pub async fn update_workspace_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceMemberParams>,
    Json(request): Json<UpdateWorkspaceMemberRequest>,
) -> ApiResult<()> {
    b2b_membership_app::update_workspace_member(
        &app_state,
        deployment_id,
        params.workspace_id,
        params.membership_id,
        request,
    )
    .await?;

    Ok(().into())
}

pub async fn remove_workspace_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceMemberParams>,
) -> ApiResult<()> {
    b2b_membership_app::remove_workspace_member(
        &app_state,
        deployment_id,
        params.workspace_id,
        params.membership_id,
    )
    .await?;
    Ok(().into())
}
