use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
};

use crate::api::multipart::{MultipartField, MultipartPayload};
use crate::application::{
    b2b_entity as b2b_entity_use_cases,
    response::{ApiErrorResponse, ApiResult},
};
use crate::middleware::RequireDeployment;
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

pub async fn create_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    multipart: Multipart,
) -> ApiResult<Organization> {
    let data = parse_entity_multipart(multipart, "organization_image", false).await?;

    let organization = b2b_entity_use_cases::create_organization(
        &app_state,
        deployment_id,
        b2b_entity_use_cases::EntityMutationInput {
            name: data.name,
            description: data.description,
            public_metadata: data.public_metadata,
            private_metadata: data.private_metadata,
            remove_image: data.remove_image,
            image_upload: data.image_upload,
        },
    )
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

    let workspace = b2b_entity_use_cases::create_workspace_for_organization(
        &app_state,
        deployment_id,
        params.organization_id,
        b2b_entity_use_cases::EntityMutationInput {
            name: data.name,
            description: data.description,
            public_metadata: data.public_metadata,
            private_metadata: data.private_metadata,
            remove_image: data.remove_image,
            image_upload: data.image_upload,
        },
    )
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

    let workspace = b2b_entity_use_cases::update_workspace(
        &app_state,
        deployment_id,
        params.workspace_id,
        b2b_entity_use_cases::EntityMutationInput {
            name: data.name,
            description: data.description,
            public_metadata: data.public_metadata,
            private_metadata: data.private_metadata,
            remove_image: data.remove_image,
            image_upload: data.image_upload,
        },
    )
    .await?;

    Ok(workspace.into())
}

pub async fn update_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
    multipart: Multipart,
) -> ApiResult<Organization> {
    let data = parse_entity_multipart(multipart, "organization_image", true).await?;

    let organization = b2b_entity_use_cases::update_organization(
        &app_state,
        deployment_id,
        params.organization_id,
        b2b_entity_use_cases::EntityMutationInput {
            name: data.name,
            description: data.description,
            public_metadata: data.public_metadata,
            private_metadata: data.private_metadata,
            remove_image: data.remove_image,
            image_upload: data.image_upload,
        },
    )
    .await?;

    Ok(organization.into())
}

pub async fn delete_organization(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<OrganizationParams>,
) -> ApiResult<()> {
    b2b_entity_use_cases::delete_organization(&app_state, deployment_id, params.organization_id)
        .await?;

    Ok(().into())
}

pub async fn delete_workspace(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<WorkspaceParams>,
) -> ApiResult<()> {
    b2b_entity_use_cases::delete_workspace(&app_state, deployment_id, params.workspace_id).await?;

    Ok(().into())
}
