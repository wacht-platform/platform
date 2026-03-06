use axum::{
    Json,
    extract::{Path, Query as QueryParams, State},
};

use crate::application::{
    b2b_query as b2b_query_use_cases,
    response::ApiResult,
};
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::{
    json::deployment_settings::DeploymentB2bSettingsUpdates, query::OrganizationListQueryParams,
};
use models::{
    DeploymentOrganizationRole, DeploymentWorkspaceRole, Organization, OrganizationDetails,
    OrganizationMemberDetails, WorkspaceDetails, WorkspaceMemberDetails,
    WorkspaceWithOrganizationName,
};

use super::{
    OrganizationMemberQueryParams, OrganizationParams, WorkspaceMemberQueryParams, WorkspaceParams,
};

pub async fn get_deployment_workspace_roles(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<crate::application::response::PaginatedResponse<DeploymentWorkspaceRole>> {
    let roles = b2b_query_use_cases::get_deployment_workspace_roles(&app_state, deployment_id).await?;
    Ok(roles.into())
}

pub async fn get_deployment_org_roles(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<crate::application::response::PaginatedResponse<DeploymentOrganizationRole>> {
    let roles = b2b_query_use_cases::get_deployment_org_roles(&app_state, deployment_id).await?;
    Ok(roles.into())
}

pub async fn update_deployment_b2b_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(settings): Json<DeploymentB2bSettingsUpdates>,
) -> ApiResult<()> {
    b2b_query_use_cases::update_deployment_b2b_settings(&app_state, deployment_id, settings).await?;
    Ok(().into())
}

pub async fn get_organization_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(query_params): QueryParams<OrganizationListQueryParams>,
) -> ApiResult<crate::application::response::PaginatedResponse<Organization>> {
    let organizations =
        b2b_query_use_cases::get_organization_list(&app_state, deployment_id, query_params).await?;
    Ok(organizations.into())
}

pub async fn get_workspace_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(query_params): QueryParams<OrganizationListQueryParams>,
) -> ApiResult<crate::application::response::PaginatedResponse<WorkspaceWithOrganizationName>> {
    let workspaces =
        b2b_query_use_cases::get_workspace_list(&app_state, deployment_id, query_params).await?;
    Ok(workspaces.into())
}

pub async fn get_organization_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
) -> ApiResult<OrganizationDetails> {
    let organization =
        b2b_query_use_cases::get_organization_details(&app_state, deployment_id, params.organization_id)
            .await?;
    Ok(organization.into())
}

pub async fn get_workspace_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
) -> ApiResult<WorkspaceDetails> {
    let workspace =
        b2b_query_use_cases::get_workspace_details(&app_state, deployment_id, params.workspace_id)
            .await?;
    Ok(workspace.into())
}

pub async fn get_organization_members(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    QueryParams(query_params): QueryParams<OrganizationMemberQueryParams>,
) -> ApiResult<crate::application::response::PaginatedResponse<OrganizationMemberDetails>> {
    let limit = query_params.limit.unwrap_or(20);
    let offset = query_params.offset.unwrap_or(0);

    let members = b2b_query_use_cases::get_organization_members(
        &app_state,
        params.organization_id,
        offset,
        limit,
        query_params.search,
        query_params.sort_key,
        query_params.sort_order,
    )
    .await?;

    Ok(members.into())
}

pub async fn get_workspace_members(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    QueryParams(query_params): QueryParams<WorkspaceMemberQueryParams>,
) -> ApiResult<crate::application::response::PaginatedResponse<WorkspaceMemberDetails>> {
    let limit = query_params.limit.unwrap_or(20);
    let offset = query_params.offset.unwrap_or(0);

    let members = b2b_query_use_cases::get_workspace_members(
        &app_state,
        params.workspace_id,
        offset,
        limit,
        query_params.search,
        query_params.sort_key,
        query_params.sort_order,
    )
    .await?;

    Ok(members.into())
}
