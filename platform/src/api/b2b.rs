use std::collections::HashMap;

use crate::middleware::RequireDeployment;
use axum::Json;
use axum::extract::{Multipart, Path, Query as QueryParams, State};
use axum::http::StatusCode;
use serde::Deserialize;

// Path parameter structs for nested routes
#[derive(Deserialize)]
pub struct OrganizationParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub organization_id: i64,
}

#[derive(Deserialize)]
pub struct OrganizationMemberParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub organization_id: i64,
    pub membership_id: i64,
}

#[derive(Deserialize)]
pub struct OrganizationRoleParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub organization_id: i64,
    pub role_id: i64,
}

#[derive(Deserialize)]
pub struct WorkspaceParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub workspace_id: i64,
}

#[derive(Deserialize)]
pub struct WorkspaceMemberParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub workspace_id: i64,
    pub membership_id: i64,
}

#[derive(Deserialize)]
pub struct PaginationParams {
    pub offset: Option<i64>,
    pub limit: Option<i32>,
}

#[derive(Deserialize)]
pub struct WorkspaceRoleParams {
    #[serde(flatten)]
    pub rest: HashMap<String, String>,
    pub workspace_id: i64,
    pub role_id: i64,
}

use crate::application::{response::ApiResult, response::PaginatedResponse};
use commands::{
    AddOrganizationMemberCommand, AddWorkspaceMemberCommand, Command, CreateOrganizationCommand,
    CreateOrganizationRoleCommand, CreateWorkspaceCommand, CreateWorkspaceRoleCommand,
    DeleteOrganizationCommand, DeleteOrganizationRoleCommand, DeleteWorkspaceCommand,
    DeleteWorkspaceRoleCommand, RemoveOrganizationMemberCommand, RemoveWorkspaceMemberCommand,
    UpdateDeploymentB2bSettingsCommand, UpdateOrganizationCommand, UpdateOrganizationMemberCommand,
    UpdateOrganizationRoleCommand, UpdateWorkspaceCommand, UpdateWorkspaceMemberCommand,
    UpdateWorkspaceRoleCommand, UploadToCdnCommand,
};
use common::state::AppState;
use dto::{
    json::{
        b2b::{
            AddOrganizationMemberRequest, AddWorkspaceMemberRequest, CreateOrganizationRoleRequest,
            CreateWorkspaceRoleRequest, UpdateOrganizationMemberRequest,
            UpdateOrganizationRoleRequest, UpdateWorkspaceMemberRequest,
            UpdateWorkspaceRoleRequest,
        },
        deployment_settings::DeploymentB2bSettingsUpdates,
    },
    query::OrganizationListQueryParams,
};
use models::{DeploymentOrganizationRole, DeploymentWorkspaceRole};
use models::{
    Organization, OrganizationDetails, OrganizationMemberDetails, OrganizationRole, Workspace,
    WorkspaceDetails, WorkspaceMemberDetails, WorkspaceRole, WorkspaceWithOrganizationName,
};
use queries::{
    DeploymentOrganizationListQuery, DeploymentWorkspaceListQuery, GetOrganizationDetailsQuery,
    GetOrganizationMembersQuery, GetWorkspaceDetailsQuery, GetWorkspaceMembersQuery,
};
use queries::{GetDeploymentOrganizationRolesQuery, GetDeploymentWorkspaceRolesQuery, Query};

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
    }.into())
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
    }.into())
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
    QueryParams(pagination): QueryParams<PaginationParams>,
) -> ApiResult<PaginatedResponse<OrganizationMemberDetails>> {
    let limit = pagination.limit.unwrap_or(20);
    let offset = pagination.offset.unwrap_or(0);

    let (members, has_more) = GetOrganizationMembersQuery::new(params.organization_id)
        .offset(offset)
        .limit(limit)
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
    QueryParams(pagination): QueryParams<PaginationParams>,
) -> ApiResult<PaginatedResponse<WorkspaceMemberDetails>> {
    let limit = pagination.limit.unwrap_or(20);
    let offset = pagination.offset.unwrap_or(0);

    let (members, has_more) = GetWorkspaceMembersQuery::new(params.workspace_id)
        .offset(offset)
        .limit(limit)
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
    if let Some(image_url) = image_url {
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

    UpdateOrganizationCommand::new(
        deployment_id,
        params.organization_id,
        name,
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

pub async fn add_organization_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    Json(request): Json<AddOrganizationMemberRequest>,
) -> ApiResult<OrganizationMemberDetails> {
    AddOrganizationMemberCommand {
        deployment_id,
        organization_id: params.organization_id,
        user_id: request.user_id,
        role_ids: request.role_ids,
    }
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
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
    .await
    .map(Into::into)
    .map_err(Into::into)
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
    UpdateOrganizationRoleCommand::new(
        deployment_id,
        params.organization_id,
        params.role_id,
        request.name,
        request.permissions,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn delete_organization_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationRoleParams>,
) -> ApiResult<()> {
    DeleteOrganizationRoleCommand::new(deployment_id, params.organization_id, params.role_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
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
    UpdateWorkspaceRoleCommand::new(
        deployment_id,
        params.workspace_id,
        params.role_id,
        request.name,
        request.permissions,
    )
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
}

pub async fn delete_workspace_role(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceRoleParams>,
) -> ApiResult<()> {
    DeleteWorkspaceRoleCommand::new(deployment_id, params.workspace_id, params.role_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

// Workspace Member Management
pub async fn add_workspace_member(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
    Json(request): Json<AddWorkspaceMemberRequest>,
) -> ApiResult<WorkspaceMemberDetails> {
    AddWorkspaceMemberCommand {
        deployment_id,
        workspace_id: params.workspace_id,
        user_id: request.user_id,
        role_ids: request.role_ids,
    }
    .execute(&app_state)
    .await
    .map(Into::into)
    .map_err(Into::into)
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
