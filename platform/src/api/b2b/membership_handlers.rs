use super::*;

pub async fn add_organization_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(request): Json<AddOrganizationMemberRequest>,
) -> ApiResult<OrganizationMemberDetails> {
    let member = AddOrganizationMemberCommand {
        deployment_id,
        organization_id: params.organization_id,
        user_id: request.user_id,
        role_ids: request.role_ids,
    }
    .execute(&app_state)
    .await?;

    let task_message = NatsTaskMessage {
        task_type: "api_key.sync_org_membership_permissions".to_string(),
        task_id: format!("api-key-org-membership-{}", member.id),
        payload: serde_json::to_value(ApiKeyOrgMembershipSyncPayload {
            membership_id: member.id,
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.api_key.sync_org_membership_permissions",
            serde_json::to_vec(&task_message)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(member.into())
}

pub async fn update_organization_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationMemberParams>,
    Json(request): Json<UpdateOrganizationMemberRequest>,
) -> ApiResult<()> {
    UpdateOrganizationMemberCommand {
        deployment_id,
        organization_id: params.organization_id,
        membership_id: params.membership_id,
        role_ids: request.role_ids,
        public_metadata: request.public_metadata,
    }
    .execute(&app_state)
    .await?;

    let task_message = NatsTaskMessage {
        task_type: "api_key.sync_org_membership_permissions".to_string(),
        task_id: format!("api-key-org-membership-{}", params.membership_id),
        payload: serde_json::to_value(ApiKeyOrgMembershipSyncPayload {
            membership_id: params.membership_id,
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.api_key.sync_org_membership_permissions",
            serde_json::to_vec(&task_message)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(().into())
}

pub async fn remove_organization_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationMemberParams>,
) -> ApiResult<()> {
    RemoveOrganizationMemberCommand {
        deployment_id,
        organization_id: params.organization_id,
        membership_id: params.membership_id,
    }
    .execute(&app_state)
    .await?;

    Ok(().into())
}

pub async fn create_organization_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(request): Json<CreateOrganizationRoleRequest>,
) -> ApiResult<OrganizationRole> {
    CreateOrganizationRoleCommand::new(
        deployment_id,
        params.organization_id,
        request.name,
        request.permissions,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn update_organization_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationRoleParams>,
    Json(request): Json<UpdateOrganizationRoleRequest>,
) -> ApiResult<OrganizationRole> {
    let role = UpdateOrganizationRoleCommand::new(
        deployment_id,
        params.organization_id,
        params.role_id,
        request.name,
        request.permissions,
    )
    .execute(&app_state)
    .await?;

    let task_message = NatsTaskMessage {
        task_type: "api_key.sync_org_role_permissions".to_string(),
        task_id: format!("api-key-org-role-{}", params.role_id),
        payload: serde_json::to_value(ApiKeyOrgRoleSyncPayload {
            role_id: params.role_id,
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.api_key.sync_org_role_permissions",
            serde_json::to_vec(&task_message)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(role.into())
}

pub async fn delete_organization_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationRoleParams>,
) -> ApiResult<()> {
    let membership_ids = GetOrganizationMembershipIdsByRoleQuery::new(params.role_id)
        .execute(&app_state)
        .await?;

    DeleteOrganizationRoleCommand::new(deployment_id, params.organization_id, params.role_id)
        .execute(&app_state)
        .await?;

    for membership_id in membership_ids {
        let task_message = NatsTaskMessage {
            task_type: "api_key.sync_org_membership_permissions".to_string(),
            task_id: format!("api-key-org-membership-{}", membership_id),
            payload: serde_json::to_value(ApiKeyOrgMembershipSyncPayload { membership_id })
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        };

        app_state
            .nats_client
            .publish(
                "worker.tasks.api_key.sync_org_membership_permissions",
                serde_json::to_vec(&task_message)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                    .into(),
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(().into())
}

// Workspace Role Management
pub async fn create_workspace_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    Json(request): Json<CreateWorkspaceRoleRequest>,
) -> ApiResult<WorkspaceRole> {
    CreateWorkspaceRoleCommand::new(
        deployment_id,
        params.workspace_id,
        request.name,
        request.permissions,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn update_workspace_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceRoleParams>,
    Json(request): Json<UpdateWorkspaceRoleRequest>,
) -> ApiResult<WorkspaceRole> {
    let role = UpdateWorkspaceRoleCommand::new(
        deployment_id,
        params.workspace_id,
        params.role_id,
        request.name,
        request.permissions,
    )
    .execute(&app_state)
    .await?;

    let task_message = NatsTaskMessage {
        task_type: "api_key.sync_workspace_role_permissions".to_string(),
        task_id: format!("api-key-workspace-role-{}", params.role_id),
        payload: serde_json::to_value(ApiKeyWorkspaceRoleSyncPayload {
            role_id: params.role_id,
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.api_key.sync_workspace_role_permissions",
            serde_json::to_vec(&task_message)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(role.into())
}

pub async fn delete_workspace_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceRoleParams>,
) -> ApiResult<()> {
    let membership_ids = GetWorkspaceMembershipIdsByRoleQuery::new(params.role_id)
        .execute(&app_state)
        .await?;

    DeleteWorkspaceRoleCommand::new(deployment_id, params.workspace_id, params.role_id)
        .execute(&app_state)
        .await?;

    for membership_id in membership_ids {
        let task_message = NatsTaskMessage {
            task_type: "api_key.sync_workspace_membership_permissions".to_string(),
            task_id: format!("api-key-workspace-membership-{}", membership_id),
            payload: serde_json::to_value(ApiKeyWorkspaceMembershipSyncPayload { membership_id })
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        };

        app_state
            .nats_client
            .publish(
                "worker.tasks.api_key.sync_workspace_membership_permissions",
                serde_json::to_vec(&task_message)
                    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                    .into(),
            )
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    Ok(().into())
}

// Workspace Member Management
pub async fn add_workspace_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    Json(request): Json<AddWorkspaceMemberRequest>,
) -> ApiResult<WorkspaceMemberDetails> {
    let member = AddWorkspaceMemberCommand {
        deployment_id,
        workspace_id: params.workspace_id,
        user_id: request.user_id,
        role_ids: request.role_ids,
    }
    .execute(&app_state)
    .await?;

    let task_message = NatsTaskMessage {
        task_type: "api_key.sync_workspace_membership_permissions".to_string(),
        task_id: format!("api-key-workspace-membership-{}", member.id),
        payload: serde_json::to_value(ApiKeyWorkspaceMembershipSyncPayload {
            membership_id: member.id,
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.api_key.sync_workspace_membership_permissions",
            serde_json::to_vec(&task_message)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(member.into())
}

pub async fn update_workspace_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceMemberParams>,
    Json(request): Json<UpdateWorkspaceMemberRequest>,
) -> ApiResult<()> {
    UpdateWorkspaceMemberCommand {
        deployment_id,
        workspace_id: params.workspace_id,
        membership_id: params.membership_id,
        role_ids: request.role_ids,
        public_metadata: request.public_metadata,
    }
    .execute(&app_state)
    .await?;

    let task_message = NatsTaskMessage {
        task_type: "api_key.sync_workspace_membership_permissions".to_string(),
        task_id: format!("api-key-workspace-membership-{}", params.membership_id),
        payload: serde_json::to_value(ApiKeyWorkspaceMembershipSyncPayload {
            membership_id: params.membership_id,
        })
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    app_state
        .nats_client
        .publish(
            "worker.tasks.api_key.sync_workspace_membership_permissions",
            serde_json::to_vec(&task_message)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(().into())
}

pub async fn remove_workspace_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceMemberParams>,
) -> ApiResult<()> {
    RemoveWorkspaceMemberCommand {
        deployment_id,
        workspace_id: params.workspace_id,
        membership_id: params.membership_id,
    }
    .execute(&app_state)
    .await?;
    Ok(().into())
}
