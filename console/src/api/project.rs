use axum::{
    extract::{Json, Multipart, Path, State},
    http::StatusCode,
};

use crate::{
    application::HttpState,
    core::{
        commands::{
            Command, CreateProductionDeploymentCommand, CreateProjectWithStagingDeploymentCommand,
            CreateStagingDeploymentCommand, DeleteDeploymentCommand, DeleteProjectCommand,
            VerifyDeploymentDnsRecordsCommand,
        },
        dto::json::project::{CreateProductionDeploymentRequest, CreateStagingDeploymentRequest},
        models::{Deployment, ProjectWithDeployments},
        queries::{GetProjectsWithDeploymentQuery, Query},
    },
};

use crate::application::response::{ApiResult, PaginatedResponse};

pub async fn get_projects(
    State(app_state): State<HttpState>,
) -> ApiResult<PaginatedResponse<ProjectWithDeployments>> {
    let projects = GetProjectsWithDeploymentQuery::new(0)
        .execute(&app_state)
        .await?;

    Ok(PaginatedResponse {
        data: projects,
        has_more: false,
    }
    .into())
}

pub async fn create_project(
    State(app_state): State<HttpState>,
    mut multipart: Multipart,
) -> ApiResult<ProjectWithDeployments> {
    let mut name = String::new();
    let mut logo_buffer: Vec<u8> = Vec::new();
    let mut methods: Vec<String> = Vec::new();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let field_name = field.name().unwrap_or_default().to_string();
        let content_type = field.content_type().unwrap_or_default().to_string();
        let value = field.bytes().await.unwrap().to_vec();

        let val_str = String::from_utf8_lossy(&value);

        if field_name == "name" {
            name = String::from_utf8_lossy(&value).into();
        } else if field_name == "methods" {
            methods.push(val_str.into());
        } else if field_name == "logo" && content_type == "image/png" {
            logo_buffer = value;
        }
    }

    if name.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Name is required").into());
    }

    CreateProjectWithStagingDeploymentCommand::new(name, logo_buffer, methods)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn create_staging_deployment(
    State(app_state): State<HttpState>,
    Path(project_id): Path<i64>,
    Json(request): Json<CreateStagingDeploymentRequest>,
) -> ApiResult<Deployment> {
    let command = CreateStagingDeploymentCommand::new(project_id, request.auth_methods)
        .execute(&app_state)
        .await?;

    Ok(command.into())
}

pub async fn create_production_deployment(
    State(app_state): State<HttpState>,
    Path(project_id): Path<i64>,
    Json(request): Json<CreateProductionDeploymentRequest>,
) -> ApiResult<Deployment> {
    let command = CreateProductionDeploymentCommand::new(
        project_id,
        request.custom_domain,
        request.auth_methods,
    )
    .execute(&app_state)
    .await?;

    Ok(command.into())
}

pub async fn verify_deployment_dns_records(
    State(app_state): State<HttpState>,
    Path(deployment_id): Path<i64>,
) -> ApiResult<Deployment> {
    VerifyDeploymentDnsRecordsCommand::new(deployment_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_project(
    State(app_state): State<HttpState>,
    Path(id): Path<i64>,
) -> ApiResult<()> {
    let command = DeleteProjectCommand::new(id, 0);
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn delete_deployment(
    State(app_state): State<HttpState>,
    Path((project_id, deployment_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    let command = DeleteDeploymentCommand::new(deployment_id, project_id);
    command.execute(&app_state).await?;

    Ok(().into())
}
