use axum::http::StatusCode;
use commands::{
    CreateOrganizationCommand, CreateWorkspaceCommand, DeleteOrganizationCommand,
    DeleteWorkspaceCommand, UpdateOrganizationCommand, UpdateWorkspaceCommand, UploadToCdnCommand,
};
use common::error::AppError;
use common::state::AppState;
use models::{Organization, Workspace};
use serde_json::Value;

use crate::application::response::ApiErrorResponse;

#[derive(Default)]
pub struct EntityMutationInput {
    pub name: Option<String>,
    pub description: Option<String>,
    pub public_metadata: Option<Value>,
    pub private_metadata: Option<Value>,
    pub remove_image: bool,
    pub image_upload: Option<(Vec<u8>, String)>,
}

async fn upload_entity_image(
    app_state: &AppState,
    deployment_id: i64,
    entity_kind: &str,
    entity_id: i64,
    image_upload: Option<(Vec<u8>, String)>,
) -> Result<Option<String>, ApiErrorResponse> {
    let Some((image_buffer, file_extension)) = image_upload else {
        return Ok(None);
    };

    let file_path = format!(
        "deployments/{}/{}/{}/logo.{}",
        deployment_id, entity_kind, entity_id, file_extension
    );
    let url = UploadToCdnCommand::new(file_path, image_buffer)
        .execute_with_deps(&app_state.s3_client)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Some(url))
}

pub async fn create_organization(
    app_state: &AppState,
    deployment_id: i64,
    data: EntityMutationInput,
) -> Result<Organization, ApiErrorResponse> {
    let name = data.name.ok_or((
        StatusCode::BAD_REQUEST,
        "Organization name is required".to_string(),
    ))?;

    let image_url = if data.image_upload.is_some() {
        let generated_id = app_state
            .sf
            .next_id()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        upload_entity_image(
            app_state,
            deployment_id,
            "organizations",
            generated_id as i64,
            data.image_upload,
        )
        .await?
    } else {
        None
    };

    CreateOrganizationCommand::new(
        deployment_id,
        name,
        data.description,
        image_url,
        data.public_metadata,
        data.private_metadata,
    )
    .with_organization_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(Into::into)
}

pub async fn create_workspace_for_organization(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    data: EntityMutationInput,
) -> Result<Workspace, ApiErrorResponse> {
    let name = data.name.ok_or((
        StatusCode::BAD_REQUEST,
        "Workspace name is required".to_string(),
    ))?;

    let image_url = if data.image_upload.is_some() {
        let generated_id = app_state
            .sf
            .next_id()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        upload_entity_image(
            app_state,
            deployment_id,
            "workspaces",
            generated_id as i64,
            data.image_upload,
        )
        .await?
    } else {
        None
    };

    CreateWorkspaceCommand::new(
        deployment_id,
        organization_id,
        name,
        data.description,
        image_url,
        data.public_metadata,
        data.private_metadata,
    )
    .with_workspace_id(
        app_state
            .sf
            .next_id()
            .map_err(|e| AppError::Internal(e.to_string()))? as i64,
    )
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(Into::into)
}

pub async fn update_workspace(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
    data: EntityMutationInput,
) -> Result<Workspace, ApiErrorResponse> {
    let image_url = upload_entity_image(
        app_state,
        deployment_id,
        "workspaces",
        workspace_id,
        data.image_upload,
    )
    .await?;

    let mut command = UpdateWorkspaceCommand::new(deployment_id, workspace_id);

    if let Some(name) = data.name {
        command = command.with_name(name);
    }
    if let Some(description) = data.description {
        command = command.with_description(Some(description));
    }
    if data.remove_image {
        command = command.with_image_url(Some(String::new()));
    } else if let Some(image_url) = image_url {
        command = command.with_image_url(Some(image_url));
    }
    if let Some(public_metadata) = data.public_metadata {
        command = command.with_public_metadata(public_metadata);
    }
    if let Some(private_metadata) = data.private_metadata {
        command = command.with_private_metadata(private_metadata);
    }

    command
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(Into::into)
}

pub async fn update_organization(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
    data: EntityMutationInput,
) -> Result<Organization, ApiErrorResponse> {
    let image_url = upload_entity_image(
        app_state,
        deployment_id,
        "organizations",
        organization_id,
        data.image_upload,
    )
    .await?;

    let final_image_url = if data.remove_image {
        Some(String::new())
    } else {
        image_url
    };

    UpdateOrganizationCommand::new(
        deployment_id,
        organization_id,
        data.name,
        data.description,
        final_image_url,
        data.public_metadata,
        data.private_metadata,
    )
    .execute_with_db(app_state.db_router.writer())
    .await
    .map_err(Into::into)
}

pub async fn delete_organization(
    app_state: &AppState,
    deployment_id: i64,
    organization_id: i64,
) -> Result<(), AppError> {
    DeleteOrganizationCommand::new(deployment_id, organization_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}

pub async fn delete_workspace(
    app_state: &AppState,
    deployment_id: i64,
    workspace_id: i64,
) -> Result<(), AppError> {
    DeleteWorkspaceCommand::new(deployment_id, workspace_id)
        .execute_with_db(app_state.db_router.writer())
        .await?;
    Ok(())
}
