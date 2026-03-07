use axum::http::StatusCode;
use commands::{
    AddOrganizationMemberCommand, AddWorkspaceMemberCommand, CreateOrganizationRoleCommand,
    CreateWorkspaceRoleCommand, DeleteOrganizationRoleCommand, DeleteWorkspaceRoleCommand,
    RemoveOrganizationMemberCommand, RemoveWorkspaceMemberCommand, UpdateOrganizationMemberCommand,
    UpdateOrganizationRoleCommand, UpdateWorkspaceMemberCommand, UpdateWorkspaceRoleCommand,
};
use common::db_router::ReadConsistency;
use common::error::AppError;
use dto::json::{
    b2b::{
        AddOrganizationMemberRequest, AddWorkspaceMemberRequest, CreateOrganizationRoleRequest,
        CreateWorkspaceRoleRequest, UpdateOrganizationMemberRequest, UpdateOrganizationRoleRequest,
        UpdateWorkspaceMemberRequest, UpdateWorkspaceRoleRequest,
    },
    nats::{
        ApiKeyOrgMembershipSyncPayload, ApiKeyOrgRoleSyncPayload,
        ApiKeyWorkspaceMembershipSyncPayload, ApiKeyWorkspaceRoleSyncPayload, NatsTaskMessage,
    },
};
use models::{OrganizationMemberDetails, OrganizationRole, WorkspaceMemberDetails, WorkspaceRole};
use queries::api_key::{
    GetOrganizationMembershipIdsByRoleQuery, GetWorkspaceMembershipIdsByRoleQuery,
};
use serde::Serialize;

use crate::application::{AppState, response::ApiErrorResponse};

pub async fn add_organization_member(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    request: AddOrganizationMemberRequest,
) -> Result<OrganizationMemberDetails, ApiErrorResponse> {
    let member = AddOrganizationMemberCommand::new(
        deployment_id,
        organization_id,
        request.user_id,
        request.role_ids,
    )
    .with_membership_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .execute_with(app_state.db_router.writer())
    .await?;

    publish_task(
        app_state,
        "worker.tasks.api_key.sync_org_membership_permissions",
        "api_key.sync_org_membership_permissions",
        format!("api-key-org-membership-{}", member.id),
        ApiKeyOrgMembershipSyncPayload {
            membership_id: member.id,
        },
    )
    .await?;

    Ok(member)
}

pub async fn update_organization_member(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    membership_id: i64,
    request: UpdateOrganizationMemberRequest,
) -> Result<(), ApiErrorResponse> {
    UpdateOrganizationMemberCommand {
        deployment_id,
        organization_id,
        membership_id,
        role_ids: request.role_ids,
        public_metadata: request.public_metadata,
    }
    .execute_with(app_state.db_router.writer())
    .await?;

    publish_task(
        app_state,
        "worker.tasks.api_key.sync_org_membership_permissions",
        "api_key.sync_org_membership_permissions",
        format!("api-key-org-membership-{}", membership_id),
        ApiKeyOrgMembershipSyncPayload { membership_id },
    )
    .await?;

    Ok(())
}

pub async fn remove_organization_member(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    membership_id: i64,
) -> Result<(), ApiErrorResponse> {
    RemoveOrganizationMemberCommand {
        deployment_id,
        organization_id,
        membership_id,
    }
    .execute_with(app_state.db_router.writer())
    .await?;
    Ok(())
}

pub async fn create_organization_role(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    request: CreateOrganizationRoleRequest,
) -> Result<OrganizationRole, ApiErrorResponse> {
    CreateOrganizationRoleCommand::new(
        deployment_id,
        organization_id,
        request.name,
        request.permissions,
    )
    .with_role_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .execute_with(app_state.db_router.writer())
    .await
    .map_err(Into::into)
}

pub async fn update_organization_role(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    role_id: i64,
    request: UpdateOrganizationRoleRequest,
) -> Result<OrganizationRole, ApiErrorResponse> {
    let role = UpdateOrganizationRoleCommand::new(
        deployment_id,
        organization_id,
        role_id,
        request.name,
        request.permissions,
    )
    .execute_with(app_state.db_router.writer())
    .await?;

    publish_task(
        app_state,
        "worker.tasks.api_key.sync_org_role_permissions",
        "api_key.sync_org_role_permissions",
        format!("api-key-org-role-{}", role_id),
        ApiKeyOrgRoleSyncPayload { role_id },
    )
    .await?;

    Ok(role)
}

pub async fn delete_organization_role(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    role_id: i64,
) -> Result<(), ApiErrorResponse> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let membership_ids = GetOrganizationMembershipIdsByRoleQuery::new(role_id)
        .execute_with(reader)
        .await?;

    DeleteOrganizationRoleCommand::new(deployment_id, organization_id, role_id)
        .execute_with(app_state.db_router.writer())
        .await?;

    for membership_id in membership_ids {
        publish_task(
            app_state,
            "worker.tasks.api_key.sync_org_membership_permissions",
            "api_key.sync_org_membership_permissions",
            format!("api-key-org-membership-{}", membership_id),
            ApiKeyOrgMembershipSyncPayload { membership_id },
        )
        .await?;
    }

    Ok(())
}

pub async fn create_workspace_role(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
    request: CreateWorkspaceRoleRequest,
) -> Result<WorkspaceRole, ApiErrorResponse> {
    CreateWorkspaceRoleCommand::new(
        deployment_id,
        workspace_id,
        request.name,
        request.permissions,
    )
    .with_role_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .execute_with(app_state.db_router.writer())
    .await
    .map_err(Into::into)
}

pub async fn update_workspace_role(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
    role_id: i64,
    request: UpdateWorkspaceRoleRequest,
) -> Result<WorkspaceRole, ApiErrorResponse> {
    let role = UpdateWorkspaceRoleCommand::new(
        deployment_id,
        workspace_id,
        role_id,
        request.name,
        request.permissions,
    )
    .execute_with(app_state.db_router.writer())
    .await?;

    publish_task(
        app_state,
        "worker.tasks.api_key.sync_workspace_role_permissions",
        "api_key.sync_workspace_role_permissions",
        format!("api-key-workspace-role-{}", role_id),
        ApiKeyWorkspaceRoleSyncPayload { role_id },
    )
    .await?;

    Ok(role)
}

pub async fn delete_workspace_role(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
    role_id: i64,
) -> Result<(), ApiErrorResponse> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let membership_ids = GetWorkspaceMembershipIdsByRoleQuery::new(role_id)
        .execute_with(reader)
        .await?;

    DeleteWorkspaceRoleCommand::new(deployment_id, workspace_id, role_id)
        .execute_with(app_state.db_router.writer())
        .await?;

    for membership_id in membership_ids {
        publish_task(
            app_state,
            "worker.tasks.api_key.sync_workspace_membership_permissions",
            "api_key.sync_workspace_membership_permissions",
            format!("api-key-workspace-membership-{}", membership_id),
            ApiKeyWorkspaceMembershipSyncPayload { membership_id },
        )
        .await?;
    }

    Ok(())
}

pub async fn add_workspace_member(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
    request: AddWorkspaceMemberRequest,
) -> Result<WorkspaceMemberDetails, ApiErrorResponse> {
    let member = AddWorkspaceMemberCommand::new(
        deployment_id,
        workspace_id,
        request.user_id,
        request.role_ids,
    )
    .with_workspace_membership_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .with_implicit_org_membership_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .execute_with(app_state.db_router.writer())
    .await?;

    publish_task(
        app_state,
        "worker.tasks.api_key.sync_workspace_membership_permissions",
        "api_key.sync_workspace_membership_permissions",
        format!("api-key-workspace-membership-{}", member.id),
        ApiKeyWorkspaceMembershipSyncPayload {
            membership_id: member.id,
        },
    )
    .await?;

    Ok(member)
}

pub async fn update_workspace_member(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
    membership_id: i64,
    request: UpdateWorkspaceMemberRequest,
) -> Result<(), ApiErrorResponse> {
    UpdateWorkspaceMemberCommand {
        deployment_id,
        workspace_id,
        membership_id,
        role_ids: request.role_ids,
        public_metadata: request.public_metadata,
    }
    .execute_with(app_state.db_router.writer())
    .await?;

    publish_task(
        app_state,
        "worker.tasks.api_key.sync_workspace_membership_permissions",
        "api_key.sync_workspace_membership_permissions",
        format!("api-key-workspace-membership-{}", membership_id),
        ApiKeyWorkspaceMembershipSyncPayload { membership_id },
    )
    .await?;

    Ok(())
}

pub async fn remove_workspace_member(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
    membership_id: i64,
) -> Result<(), ApiErrorResponse> {
    RemoveWorkspaceMemberCommand {
        deployment_id,
        workspace_id,
        membership_id,
    }
    .execute_with(app_state.db_router.writer())
    .await?;
    Ok(())
}

async fn publish_task<T>(
    app_state: &AppState,
    subject: &'static str,
    task_type: &str,
    task_id: String,
    payload: T,
) -> Result<(), ApiErrorResponse>
where
    T: Serialize,
{
    let task_message = NatsTaskMessage {
        task_type: task_type.to_string(),
        task_id,
        payload: serde_json::to_value(payload)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
    };

    app_state
        .nats_client
        .publish(
            subject,
            serde_json::to_vec(&task_message)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                .into(),
        )
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(())
}
