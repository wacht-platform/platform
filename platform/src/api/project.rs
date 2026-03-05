use axum::extract::{Json, Multipart, Path, State};
use common::db_router::ReadConsistency;
use wacht::middleware::extractors::RequireAuth;

use crate::api::multipart::MultipartPayload;
use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use commands::{
    Command, CreateProductionDeploymentCommand, CreateProjectWithStagingDeploymentCommand,
    CreateStagingDeploymentCommand, VerifyDeploymentDnsRecordsCommand,
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
    .with_consistency(ReadConsistency::Eventual)
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
    multipart: Multipart,
) -> ApiResult<ProjectWithDeployments> {
    let payload = MultipartPayload::parse(multipart).await?;
    let name = payload.required_text("name")?;
    let methods = payload.repeated_text("methods")?;

    let owner_id = auth
        .organization_id
        .as_ref()
        .map(|id| format!("org_{id}"))
        .unwrap_or_else(|| format!("user_{}", auth.user_id));

    let project = CreateProjectWithStagingDeploymentCommand::new(name, methods)
        .with_owner(owner_id)
        .execute(&app_state)
        .await?;

    Ok(project.into())
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
