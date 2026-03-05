use super::*;
use crate::api::multipart::{MultipartField, MultipartPayload};
use crate::application::response::ApiErrorResponse;

fn parse_metadata_field(
    field: &MultipartField,
    label: &str,
) -> Result<Option<serde_json::Value>, ApiErrorResponse> {
    let metadata_str = field.text()?;
    if metadata_str.trim().is_empty() {
        return Ok(None);
    }

    serde_json::from_str(&metadata_str)
        .map(Some)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid {} metadata JSON: {}", label, e),
            )
                .into()
        })
}

fn parse_image_upload(
    field: &MultipartField,
) -> Result<Option<(Vec<u8>, String)>, ApiErrorResponse> {
    let Some(file_extension) = field.image_extension()? else {
        return Ok(None);
    };

    if field.bytes.is_empty() {
        return Ok(None);
    }

    Ok(Some((field.bytes.clone(), file_extension.to_string())))
}

pub async fn create_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    multipart: Multipart,
) -> ApiResult<Organization> {
    let mut name = String::new();
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;

    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "name" => {
                name = field.text()?;
            }
            "description" => {
                let desc = field.text()?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "public")? {
                    public_metadata = Some(metadata);
                }
            }
            "private_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "private")? {
                    private_metadata = Some(metadata);
                }
            }
            "organization_image" => {
                if let Some((image_buffer, file_extension)) = parse_image_upload(field)? {
                    // Generate unique organization ID for file path
                    let org_id = app_state
                        .sf
                        .next_id()
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    let file_path = format!(
                        "deployments/{}/organizations/{}/logo.{}",
                        deployment_id, org_id, file_extension
                    );

                    let url = UploadToCdnCommand::new(file_path, image_buffer)
                        .execute(&app_state)
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                    image_url = Some(url);
                }
            }
            _ => {
                // Skip unknown fields
            }
        }
    }

    // Validate required fields
    if name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Organization name is required".to_string(),
        )
            .into());
    }

    CreateOrganizationCommand::new(
        deployment_id,
        name.trim().to_string(),
        description,
        image_url,
        public_metadata,
        private_metadata,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn create_workspace_for_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    multipart: Multipart,
) -> ApiResult<Workspace> {
    let mut name = String::new();
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;

    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "name" => {
                name = field.text()?;
            }
            "description" => {
                let desc = field.text()?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "public")? {
                    public_metadata = Some(metadata);
                }
            }
            "private_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "private")? {
                    private_metadata = Some(metadata);
                }
            }
            "workspace_image" => {
                if let Some((image_buffer, file_extension)) = parse_image_upload(field)? {
                    // Generate unique workspace ID for file path
                    let workspace_id = app_state
                        .sf
                        .next_id()
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    let file_path = format!(
                        "deployments/{}/workspaces/{}/logo.{}",
                        deployment_id, workspace_id, file_extension
                    );

                    let url = UploadToCdnCommand::new(file_path, image_buffer)
                        .execute(&app_state)
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                    image_url = Some(url);
                }
            }
            _ => {
                // Skip unknown fields
            }
        }
    }

    // Validate required fields
    if name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Workspace name is required".to_string(),
        )
            .into());
    }

    CreateWorkspaceCommand::new(
        deployment_id,
        params.organization_id,
        name.trim().to_string(),
        description,
        image_url,
        public_metadata,
        private_metadata,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn update_workspace(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    multipart: Multipart,
) -> ApiResult<Workspace> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;
    let mut remove_image = false;

    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "name" => {
                let workspace_name = field.text()?;
                if !workspace_name.trim().is_empty() {
                    name = Some(workspace_name.trim().to_string());
                }
            }
            "description" => {
                let desc = field.text()?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "public")? {
                    public_metadata = Some(metadata);
                }
            }
            "private_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "private")? {
                    private_metadata = Some(metadata);
                }
            }
            "remove_image" => {
                let value = field.text()?;
                remove_image = value == "true";
            }
            "workspace_image" => {
                if let Some((image_buffer, file_extension)) = parse_image_upload(field)? {
                    let file_path = format!(
                        "deployments/{}/workspaces/{}/logo.{}",
                        deployment_id, params.workspace_id, file_extension
                    );

                    let url = UploadToCdnCommand::new(file_path, image_buffer)
                        .execute(&app_state)
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                    image_url = Some(url);
                }
            }
            _ => {
                // Skip unknown fields
            }
        }
    }

    let mut command = UpdateWorkspaceCommand::new(deployment_id, params.workspace_id);

    if let Some(name) = name {
        command = command.with_name(name);
    }
    if let Some(description) = description {
        command = command.with_description(Some(description));
    }
    // Handle image removal - set to empty string
    if remove_image {
        command = command.with_image_url(Some(String::new()));
    } else if let Some(image_url) = image_url {
        command = command.with_image_url(Some(image_url));
    }
    if let Some(public_metadata) = public_metadata {
        command = command.with_public_metadata(public_metadata);
    }
    if let Some(private_metadata) = private_metadata {
        command = command.with_private_metadata(private_metadata);
    }

    command
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn update_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    multipart: Multipart,
) -> ApiResult<Organization> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;
    let mut remove_image = false;

    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        match field.name.as_str() {
            "name" => {
                let org_name = field.text()?;
                if !org_name.trim().is_empty() {
                    name = Some(org_name.trim().to_string());
                }
            }
            "description" => {
                let desc = field.text()?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "public")? {
                    public_metadata = Some(metadata);
                }
            }
            "private_metadata" => {
                if let Some(metadata) = parse_metadata_field(field, "private")? {
                    private_metadata = Some(metadata);
                }
            }
            "remove_image" => {
                let value = field.text()?;
                remove_image = value == "true";
            }
            "organization_image" => {
                if let Some((image_buffer, file_extension)) = parse_image_upload(field)? {
                    let file_path = format!(
                        "deployments/{}/organizations/{}/logo.{}",
                        deployment_id, params.organization_id, file_extension
                    );

                    let url = UploadToCdnCommand::new(file_path, image_buffer)
                        .execute(&app_state)
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

                    image_url = Some(url);
                }
            }
            _ => {
                // Skip unknown fields
            }
        }
    }

    // Handle image removal - set to empty string
    let final_image_url = if remove_image {
        Some(String::new())
    } else {
        image_url
    };

    UpdateOrganizationCommand::new(
        deployment_id,
        params.organization_id,
        name,
        description,
        final_image_url,
        public_metadata,
        private_metadata,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
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
