use super::*;

pub async fn get_deployment_workspace_roles(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentWorkspaceRole>> {
    GetDeploymentWorkspaceRolesQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map(PaginatedResponse::from)
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_deployment_org_roles(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<PaginatedResponse<DeploymentOrganizationRole>> {
    GetDeploymentOrganizationRolesQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map(PaginatedResponse::from)
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_deployment_b2b_settings(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(settings): Json<DeploymentB2bSettingsUpdates>,
) -> ApiResult<()> {
    UpdateDeploymentB2bSettingsCommand::new(deployment_id, settings)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_organization_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(query_params): QueryParams<OrganizationListQueryParams>,
) -> ApiResult<PaginatedResponse<Organization>> {
    let limit = query_params.limit.unwrap_or(10);

    let organizations = DeploymentOrganizationListQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(query_params.offset.unwrap_or(0))
        .sort_key(query_params.sort_key)
        .sort_order(query_params.sort_order)
        .search(query_params.search)
        .execute(&app_state)
        .await?;

    let has_more = organizations.len() > limit as usize;
    let organizations = if has_more {
        organizations[..limit as usize].to_vec()
    } else {
        organizations
    };

    Ok(PaginatedResponse {
        data: organizations,
        has_more,
        limit: Some(limit),
        offset: Some(query_params.offset.unwrap_or(0) as i32),
    }
    .into())
}

pub async fn get_workspace_list(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    QueryParams(query_params): QueryParams<OrganizationListQueryParams>,
) -> ApiResult<PaginatedResponse<WorkspaceWithOrganizationName>> {
    let limit = query_params.limit.unwrap_or(10);

    let workspaces = DeploymentWorkspaceListQuery::new(deployment_id)
        .limit(limit + 1)
        .offset(query_params.offset.unwrap_or(0))
        .sort_key(query_params.sort_key)
        .sort_order(query_params.sort_order)
        .search(query_params.search)
        .execute(&app_state)
        .await?;

    let has_more = workspaces.len() > limit as usize;
    let workspaces = if has_more {
        workspaces[..limit as usize].to_vec()
    } else {
        workspaces
    };

    Ok(PaginatedResponse {
        data: workspaces,
        has_more,
        limit: Some(limit),
        offset: Some(query_params.offset.unwrap_or(0) as i32),
    }
    .into())
}

pub async fn get_organization_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
) -> ApiResult<OrganizationDetails> {
    GetOrganizationDetailsQuery::new(deployment_id, params.organization_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_workspace_details(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
) -> ApiResult<WorkspaceDetails> {
    GetWorkspaceDetailsQuery::new(deployment_id, params.workspace_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn get_organization_members(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    QueryParams(query_params): QueryParams<OrganizationMemberQueryParams>,
) -> ApiResult<PaginatedResponse<OrganizationMemberDetails>> {
    let limit = query_params.limit.unwrap_or(20);
    let offset = query_params.offset.unwrap_or(0);

    let (members, has_more) = GetOrganizationMembersQuery::new(params.organization_id)
        .offset(offset)
        .limit(limit)
        .search(query_params.search)
        .sort_key(query_params.sort_key)
        .sort_order(query_params.sort_order)
        .execute(&app_state)
        .await?;

    Ok(PaginatedResponse {
        data: members,
        has_more,
        limit: Some(limit as i32),
        offset: Some(offset as i32),
    }
    .into())
}

pub async fn get_workspace_members(
    State(app_state): State<AppState>,
    RequireDeployment(_): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    QueryParams(query_params): QueryParams<WorkspaceMemberQueryParams>,
) -> ApiResult<PaginatedResponse<WorkspaceMemberDetails>> {
    let limit = query_params.limit.unwrap_or(20);
    let offset = query_params.offset.unwrap_or(0);

    let (members, has_more) = GetWorkspaceMembersQuery::new(params.workspace_id)
        .offset(offset)
        .limit(limit)
        .search(query_params.search)
        .sort_key(query_params.sort_key)
        .sort_order(query_params.sort_order)
        .execute(&app_state)
        .await?;

    Ok(PaginatedResponse {
        data: members,
        has_more,
        limit: Some(limit as i32),
        offset: Some(offset as i32),
    }
    .into())
}
