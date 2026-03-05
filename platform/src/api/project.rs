use axum::extract::{Json, Multipart, Path, State};
use wacht::middleware::extractors::RequireAuth;

use crate::api::multipart::MultipartPayload;
use crate::application::project::{
    CreateProductionDeploymentInput, CreateProjectWithStagingInput, CreateStagingDeploymentInput,
    GetProjectsInput, VerifyDeploymentDnsRecordsInput,
    create_production_deployment as run_create_production_deployment, create_project_with_staging,
    create_staging_deployment as run_create_staging_deployment, get_projects as run_get_projects,
    verify_deployment_dns_records as run_verify_deployment_dns_records,
};
use crate::application::response::{ApiResult, PaginatedResponse};
use common::state::AppState;

use dto::json::project::{CreateProductionDeploymentRequest, CreateStagingDeploymentRequest};
use models::{Deployment, ProjectWithDeployments};

pub async fn get_projects(
    State(app_state): State<AppState>,
    RequireAuth(auth): RequireAuth,
) -> ApiResult<PaginatedResponse<ProjectWithDeployments>> {
    let input = GetProjectsInput::new(auth.user_id, auth.organization_id);
    let projects = run_get_projects(&app_state, input).await?;

    Ok(PaginatedResponse::from(projects).into())
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

    let input = CreateProjectWithStagingInput::new(name, methods, owner_id);
    let project = create_project_with_staging(&app_state, input).await?;

    Ok(project.into())
}

pub async fn create_staging_deployment(
    State(app_state): State<AppState>,
    Path(project_id): Path<i64>,
    Json(request): Json<CreateStagingDeploymentRequest>,
) -> ApiResult<Deployment> {
    let input = CreateStagingDeploymentInput::new(project_id, request.auth_methods);
    let command = run_create_staging_deployment(&app_state, input).await?;

    Ok(command.into())
}

pub async fn create_production_deployment(
    State(app_state): State<AppState>,
    Path(project_id): Path<i64>,
    Json(request): Json<CreateProductionDeploymentRequest>,
) -> ApiResult<Deployment> {
    let input = CreateProductionDeploymentInput::new(
        project_id,
        request.custom_domain,
        request.auth_methods,
    );
    let command = run_create_production_deployment(&app_state, input).await?;

    Ok(command.into())
}

pub async fn verify_deployment_dns_records(
    State(app_state): State<AppState>,
    Path(deployment_id): Path<i64>,
) -> ApiResult<Deployment> {
    let input = VerifyDeploymentDnsRecordsInput::new(deployment_id);
    let deployment = run_verify_deployment_dns_records(&app_state, input).await?;
    Ok(deployment.into())
}
