use super::*;

pub async fn create_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    mut multipart: Multipart,
) -> ApiResult<Organization> {
    let mut name = String::new();
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;

    // Parse multipart form data
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "name" => {
                name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "description" => {
                let desc = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    public_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid public metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "private_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    private_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid private metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "organization_image" => {
                let content_type = field.content_type().unwrap_or_default().to_string();

                if content_type.starts_with("image/") {
                    let file_extension = if content_type == "image/jpeg"
                        || content_type == "image/jpg"
                    {
                        "jpg"
                    } else if content_type == "image/png" {
                        "png"
                    } else if content_type == "image/gif" {
                        "gif"
                    } else if content_type == "image/webp" {
                        "webp"
                    } else if content_type == "image/x-icon"
                        || content_type == "image/vnd.microsoft.icon"
                    {
                        "ico"
                    } else {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO".to_string(),
                        ).into());
                    };

                    let image_buffer = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                        .to_vec();

                    if !image_buffer.is_empty() {
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
    mut multipart: Multipart,
) -> ApiResult<Workspace> {
    let mut name = String::new();
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;

    // Parse multipart form data
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "name" => {
                name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
            }
            "description" => {
                let desc = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    public_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid public metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "private_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    private_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid private metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "workspace_image" => {
                let content_type = field.content_type().unwrap_or_default().to_string();

                if content_type.starts_with("image/") {
                    let file_extension = if content_type == "image/jpeg"
                        || content_type == "image/jpg"
                    {
                        "jpg"
                    } else if content_type == "image/png" {
                        "png"
                    } else if content_type == "image/gif" {
                        "gif"
                    } else if content_type == "image/webp" {
                        "webp"
                    } else if content_type == "image/x-icon"
                        || content_type == "image/vnd.microsoft.icon"
                    {
                        "ico"
                    } else {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO".to_string(),
                        ).into());
                    };

                    let image_buffer = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                        .to_vec();

                    if !image_buffer.is_empty() {
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
    mut multipart: Multipart,
) -> ApiResult<Workspace> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;
    let mut remove_image = false;

    // Parse multipart form data
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "name" => {
                let workspace_name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !workspace_name.trim().is_empty() {
                    name = Some(workspace_name.trim().to_string());
                }
            }
            "description" => {
                let desc = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    public_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid public metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "private_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    private_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid private metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "remove_image" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                remove_image = value == "true";
            }
            "workspace_image" => {
                let content_type = field.content_type().unwrap_or_default().to_string();

                if content_type.starts_with("image/") {
                    let file_extension = if content_type == "image/jpeg"
                        || content_type == "image/jpg"
                    {
                        "jpg"
                    } else if content_type == "image/png" {
                        "png"
                    } else if content_type == "image/gif" {
                        "gif"
                    } else if content_type == "image/webp" {
                        "webp"
                    } else if content_type == "image/x-icon"
                        || content_type == "image/vnd.microsoft.icon"
                    {
                        "ico"
                    } else {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO".to_string(),
                        ).into());
                    };

                    let image_buffer = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                        .to_vec();

                    if !image_buffer.is_empty() {
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
    mut multipart: Multipart,
) -> ApiResult<Organization> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut image_url: Option<String> = None;
    let mut public_metadata: Option<serde_json::Value> = None;
    let mut private_metadata: Option<serde_json::Value> = None;
    let mut remove_image = false;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let field_name = field.name().unwrap_or_default().to_string();

        match field_name.as_str() {
            "name" => {
                let org_name = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !org_name.trim().is_empty() {
                    name = Some(org_name.trim().to_string());
                }
            }
            "description" => {
                let desc = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !desc.trim().is_empty() {
                    description = Some(desc.trim().to_string());
                }
            }
            "public_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    public_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid public metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "private_metadata" => {
                let metadata_str = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                if !metadata_str.trim().is_empty() {
                    private_metadata = Some(serde_json::from_str(&metadata_str).map_err(|e| {
                        (
                            StatusCode::BAD_REQUEST,
                            format!("Invalid private metadata JSON: {}", e),
                        )
                    })?);
                }
            }
            "remove_image" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
                remove_image = value == "true";
            }
            "organization_image" => {
                let content_type = field.content_type().unwrap_or_default().to_string();

                if content_type.starts_with("image/") {
                    let file_extension = if content_type == "image/jpeg"
                        || content_type == "image/jpg"
                    {
                        "jpg"
                    } else if content_type == "image/png" {
                        "png"
                    } else if content_type == "image/gif" {
                        "gif"
                    } else if content_type == "image/webp" {
                        "webp"
                    } else if content_type == "image/x-icon"
                        || content_type == "image/vnd.microsoft.icon"
                    {
                        "ico"
                    } else {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO".to_string(),
                        ).into());
                    };

                    let image_buffer = field
                        .bytes()
                        .await
                        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
                        .to_vec();

                    if !image_buffer.is_empty() {
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

