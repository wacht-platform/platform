use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
};

use crate::api::multipart::{MultipartField, MultipartPayload};
use crate::application::response::{ApiErrorResponse, ApiResult};
use crate::middleware::RequireDeployment;
use commands::{
    Command, CreateOrganizationCommand, CreateWorkspaceCommand, DeleteOrganizationCommand,
    DeleteWorkspaceCommand, UpdateOrganizationCommand, UpdateWorkspaceCommand, UploadToCdnCommand,
};
use common::state::AppState;
use models::{Organization, Workspace};
use serde_json::Value;

use super::{OrganizationParams, WorkspaceParams};

#[derive(Default)]
struct EntityMultipartData {
    name: Option<String>,
    description: Option<String>,
    public_metadata: Option<Value>,
    private_metadata: Option<Value>,
    remove_image: bool,
    image_upload: Option<(Vec<u8>, String)>,
}

fn parse_metadata_field(
    field: &MultipartField,
    label: &str,
) -> Result<Option<Value>, ApiErrorResponse> {
    let metadata_str = field.text()?;
    if metadata_str.trim().is_empty() {
        return Ok(None);
    }

    serde_json::from_str(&metadata_str).map(Some).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Invalid {} metadata JSON: {}", label, e),
        )
            .into()
    })
}

async fn parse_entity_multipart(
    multipart: Multipart,
    image_field_name: &str,
    allow_remove_image: bool,
) -> Result<EntityMultipartData, ApiErrorResponse> {
    let payload = MultipartPayload::parse(multipart).await?;
    let mut data = EntityMultipartData::default();

    for field in payload.fields() {
        match field.name.as_str() {
            "name" => {
                data.name = field.optional_text_trimmed()?;
            }
            "description" => {
                data.description = field.optional_text_trimmed()?;
            }
            "public_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "public")? {
                    data.public_metadata = Some(metadata);
                }
            }
            "private_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "private")? {
                    data.private_metadata = Some(metadata);
                }
            }
            "remove_image" if allow_remove_image => {
                data.remove_image = field.bool_true()?;
            }
            _ if field.name == image_field_name => {
                if let Some(image) = field.image_upload()? {
                    data.image_upload = Some(image);
                }
            }
            _ => {}
        }
    }

    Ok(data)
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
        .execute(app_state)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Some(url))
}

pub async fn create_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    multipart: Multipart,
) -> ApiResult<Organization> {
    let data = parse_entity_multipart(multipart, "organization_image", false).await?;
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
            &app_state,
            deployment_id,
            "organizations",
            generated_id as i64,
            data.image_upload,
        )
        .await?
    } else {
        None
    };

    let organization = CreateOrganizationCommand::new(
        deployment_id,
        name,
        data.description,
        image_url,
        data.public_metadata,
        data.private_metadata,
    )
    .execute(&app_state)
    .await?;
    Ok(organization.into())
}

pub async fn create_workspace_for_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    multipart: Multipart,
) -> ApiResult<Workspace> {
    let data = parse_entity_multipart(multipart, "workspace_image", false).await?;
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
            &app_state,
            deployment_id,
            "workspaces",
            generated_id as i64,
            data.image_upload,
        )
        .await?
    } else {
        None
    };

    let workspace = CreateWorkspaceCommand::new(
        deployment_id,
        params.organization_id,
        name,
        data.description,
        image_url,
        data.public_metadata,
        data.private_metadata,
    )
    .execute(&app_state)
    .await?;
    Ok(workspace.into())
}

pub async fn update_workspace(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    multipart: Multipart,
) -> ApiResult<Workspace> {
    let data = parse_entity_multipart(multipart, "workspace_image", true).await?;
    let image_url = upload_entity_image(
        &app_state,
        deployment_id,
        "workspaces",
        params.workspace_id,
        data.image_upload,
    )
    .await?;

    let mut command = UpdateWorkspaceCommand::new(deployment_id, params.workspace_id);

    if let Some(name) = data.name {
        command = command.with_name(name);
    }
    if let Some(description) = data.description {
        command = command.with_description(Some(description));
    }
    // Handle image removal - set to empty string
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

    let workspace = command.execute(&app_state).await?;
    Ok(workspace.into())
}

pub async fn update_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    multipart: Multipart,
) -> ApiResult<Organization> {
    let data = parse_entity_multipart(multipart, "organization_image", true).await?;
    let image_url = upload_entity_image(
        &app_state,
        deployment_id,
        "organizations",
        params.organization_id,
        data.image_upload,
    )
    .await?;

    // Handle image removal - set to empty string
    let final_image_url = if data.remove_image {
        Some(String::new())
    } else {
        image_url
    };

    let organization = UpdateOrganizationCommand::new(
        deployment_id,
        params.organization_id,
        data.name,
        data.description,
        final_image_url,
        data.public_metadata,
        data.private_metadata,
    )
    .execute(&app_state)
    .await?;
    Ok(organization.into())
}

pub async fn delete_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
) -> ApiResult<()> {
    DeleteOrganizationCommand::new(deployment_id, params.organization_id)
        .execute(&app_state)
        .await?;

    Ok(().into())
}

pub async fn delete_workspace(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
) -> ApiResult<()> {
    DeleteWorkspaceCommand::new(deployment_id, params.workspace_id)
        .execute(&app_state)
        .await?;

    Ok(().into())
}

// Organization Member Management
