use crate::{api::pagination::paginate_results, application::response::PaginatedResponse};
use commands::UpdateDeploymentB2bSettingsCommand;
use common::db_router::ReadConsistency;
use common::deps;
use common::error::AppError;
use common::state::AppState;
use dto::{
    json::deployment_settings::DeploymentB2bSettingsUpdates, query::OrganizationListQueryParams,
};
use models::{
    Organization, OrganizationDetails, OrganizationMemberDetails, OrganizationRole,
    WorkspaceDetails, WorkspaceMemberDetails, WorkspaceRole, WorkspaceWithOrganizationName,
};
use queries::{
    DeploymentOrganizationListQuery, DeploymentWorkspaceListQuery, GetOrganizationDetailsQuery,
    GetOrganizationMembersQuery, GetWorkspaceDetailsQuery, GetWorkspaceMembersQuery,
};

fn paginated_with_has_more<T>(
    data: Vec<T>,
    has_more: bool,
    limit: i32,
    offset: i64,
) -> PaginatedResponse<T>
where
    T: serde::Serialize,
{
    PaginatedResponse {
        data,
        has_more,
        limit: Some(limit),
        offset: Some(offset as i32),
    }
}

pub async fn get_workspace_roles(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
) -> Result<PaginatedResponse<WorkspaceRole>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let workspace = GetWorkspaceDetailsQuery::new(deployment_id, workspace_id)
        .execute_with_db(reader)
        .await?;
    Ok(PaginatedResponse::from(workspace.roles))
}

pub async fn get_organization_roles(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
) -> Result<PaginatedResponse<OrganizationRole>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let organization = GetOrganizationDetailsQuery::new(deployment_id, organization_id)
        .execute_with_db(reader)
        .await?;
    Ok(PaginatedResponse::from(organization.roles))
}

pub async fn update_deployment_b2b_settings(
    app_state: &AppState,
    deployment_id: i64,
    settings: DeploymentB2bSettingsUpdates,
) -> Result<(), AppError> {
    let deps = deps::from_app(app_state).db().redis();
    UpdateDeploymentB2bSettingsCommand::new(deployment_id, settings)
        .execute_with_deps(&deps)
        .await?;
    Ok(())
}

pub async fn get_organization_list(
    app_state: &AppState,
    deployment_id: i64,
    query_params: OrganizationListQueryParams,
) -> Result<PaginatedResponse<Organization>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let limit = query_params.limit.unwrap_or(10);
    let offset = query_params.offset.unwrap_or(0);

    let organizations = DeploymentOrganizationListQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(query_params.sort_key)
        .sort_order(query_params.sort_order)
        .search(query_params.search)
        .execute_with_db(reader)
        .await?;

    Ok(paginate_results(organizations, limit, Some(offset)))
}

pub async fn get_workspace_list(
    app_state: &AppState,
    deployment_id: i64,
    query_params: OrganizationListQueryParams,
) -> Result<PaginatedResponse<WorkspaceWithOrganizationName>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let limit = query_params.limit.unwrap_or(10);
    let offset = query_params.offset.unwrap_or(0);

    let workspaces = DeploymentWorkspaceListQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(offset)
        .sort_key(query_params.sort_key)
        .sort_order(query_params.sort_order)
        .search(query_params.search)
        .execute_with_db(reader)
        .await?;

    Ok(paginate_results(workspaces, limit, Some(offset)))
}

pub async fn get_organization_details(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
) -> Result<OrganizationDetails, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetOrganizationDetailsQuery::new(deployment_id, organization_id)
        .execute_with_db(reader)
        .await
}

pub async fn get_workspace_details(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
) -> Result<WorkspaceDetails, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    GetWorkspaceDetailsQuery::new(deployment_id, workspace_id)
        .execute_with_db(reader)
        .await
}

pub async fn get_organization_members(
    app_state: &AppState,
    organization_id: i64,
    offset: i64,
    limit: i32,
    search: Option<String>,
    sort_key: Option<String>,
    sort_order: Option<String>,
) -> Result<PaginatedResponse<OrganizationMemberDetails>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let (members, has_more) = GetOrganizationMembersQuery::new(organization_id)
        .offset(offset)
        .limit(limit)
        .search(search)
        .sort_key(sort_key)
        .sort_order(sort_order)
        .execute_with_db(reader)
        .await?;

    Ok(paginated_with_has_more(members, has_more, limit, offset))
}

pub async fn get_workspace_members(
    app_state: &AppState,
    workspace_id: i64,
    offset: i64,
    limit: i32,
    search: Option<String>,
    sort_key: Option<String>,
    sort_order: Option<String>,
) -> Result<PaginatedResponse<WorkspaceMemberDetails>, AppError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let (members, has_more) = GetWorkspaceMembersQuery::new(workspace_id)
        .offset(offset)
        .limit(limit)
        .search(search)
        .sort_key(sort_key)
        .sort_order(sort_order)
        .execute_with_db(reader)
        .await?;

    Ok(paginated_with_has_more(members, has_more, limit, offset))
}
