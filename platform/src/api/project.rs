use axum::{
    extract::{Json, Multipart, Path, State},
    http::StatusCode,
};
use wacht::middleware::extractors::RequireAuth;

use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{
    Command, CreateProductionDeploymentCommand, CreateProjectWithStagingDeploymentCommand,
    CreateStagingDeploymentCommand, DeleteDeploymentCommand, DeleteProjectCommand,
    VerifyDeploymentDnsRecordsCommand,
};
use dto::json::project::{CreateProductionDeploymentRequest, CreateStagingDeploymentRequest};
use models::{Deployment, ProjectWithDeployments};
use queries::{GetProjectsWithDeploymentQuery, Query};

pub async fn get_projects(
    State(app_state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PaginatedResponse<ProjectWithDeployments>> {
    let projects = GetProjectsWithDeploymentQuery::for_user_or_organization(
        auth.user_id,
        auth.organization_id,
    )
    .execute(&app_state)
    .await?;

    Ok(PaginatedResponse {
        data: projects,
        has_more: false,
        limit: None,
        offset: None,
    }
    .into())
}

pub async fn create_project(
    State(app_state): State<AppState>,
    RequireAuth(auth): RequireAuth,
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

    let owner_id = auth
        .organization_id
        .clone()
        .map(|id| format!("org_{id}"))
        .unwrap_or(format!("user_{}", auth.user_id));

    CreateProjectWithStagingDeploymentCommand::new(name, logo_buffer, methods)
        .with_owner(owner_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn create_staging_deployment(
    State(app_state): State<AppState>,
    Path(project_id): Path<i64>,
    Json(request): Json<CreateStagingDeploymentRequest>,
) -> ApiResult<Deployment> {
    let command = CreateStagingDeploymentCommand::new(project_id, request.auth_methods)
        .execute(&app_state)
        .await?;

    Ok(command.into())
}

pub async fn create_production_deployment(
    State(app_state): State<AppState>,
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
    State(app_state): State<AppState>,
    Path(deployment_id): Path<i64>,
) -> ApiResult<Deployment> {
    VerifyDeploymentDnsRecordsCommand::new(deployment_id)
        .execute(&app_state)
        .await
        .map(Into::into)
        .map_err(Into::into)
}

pub async fn delete_project(
    State(app_state): State<AppState>,
    RequireAuth(auth): RequireAuth,
    Path(id): Path<i64>,
) -> ApiResult<()> {
    let projects = GetProjectsWithDeploymentQuery::for_user_or_organization(
        auth.user_id.clone(),
        auth.organization_id.clone(),
    )
    .execute(&app_state)
    .await?;

    if !projects.iter().any(|p| p.id == id) {
        return Err((
            StatusCode::FORBIDDEN,
            "You don't have permission to delete this project",
        )
            .into());
    }

    let command = DeleteProjectCommand::new(id, 0);
    command.execute(&app_state).await?;

    Ok(().into())
}

pub async fn delete_deployment(
    State(app_state): State<AppState>,
    Path((project_id, deployment_id)): Path<(i64, i64)>,
) -> ApiResult<()> {
    let command = DeleteDeploymentCommand::new(deployment_id, project_id);
    command.execute(&app_state).await?;

    Ok(().into())
}
