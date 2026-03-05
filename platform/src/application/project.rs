use commands::{
    AppStateIdGenerator, CreateProductionDeploymentCommand, CreateProjectWithStagingDeploymentCommand,
    ProductionDeploymentDeps,
    CreateStagingDeploymentCommand, DeleteProjectCommand, VerifyDeploymentDnsDeps,
    VerifyDeploymentDnsRecordsCommand,
};
use common::db_router::ReadConsistency;

use crate::application::{AppError, AppState};
use models::{Deployment, ProjectWithDeployments};
use queries::GetProjectsWithDeploymentQuery;

pub struct GetProjectsInput {
    user_id: String,
    organization_id: Option<String>,
}

impl GetProjectsInput {
    pub fn new(user_id: String, organization_id: Option<String>) -> Self {
        Self {
            user_id,
            organization_id,
        }
    }
}

pub async fn get_projects(
    app_state: &AppState,
    input: GetProjectsInput,
) -> Result<Vec<ProjectWithDeployments>, AppError> {
    GetProjectsWithDeploymentQuery::for_user_or_organization(input.user_id, input.organization_id)
        .with_consistency(ReadConsistency::Eventual)
        .execute_with(app_state)
        .await
}

pub struct CreateProjectWithStagingInput {
    name: String,
    auth_methods: Vec<String>,
    owner_id: String,
}

impl CreateProjectWithStagingInput {
    pub fn new(name: String, auth_methods: Vec<String>, owner_id: String) -> Self {
        Self {
            name,
            auth_methods,
            owner_id,
        }
    }
}

pub async fn create_project_with_staging(
    app_state: &AppState,
    input: CreateProjectWithStagingInput,
) -> Result<ProjectWithDeployments, AppError> {
    let command = CreateProjectWithStagingDeploymentCommand::builder()
        .name(input.name)
        .auth_methods(input.auth_methods)
        .owner_id(input.owner_id)
        .build()?;
    let mut tx = app_state.db_router.writer().begin().await?;
    let ids = AppStateIdGenerator::new(app_state);
    let project = command.execute_in_tx(&ids, &mut tx).await?;
    tx.commit().await?;
    Ok(project)
}

pub struct CreateStagingDeploymentInput {
    project_id: i64,
    auth_methods: Vec<String>,
}

impl CreateStagingDeploymentInput {
    pub fn new(project_id: i64, auth_methods: Vec<String>) -> Self {
        Self {
            project_id,
            auth_methods,
        }
    }
}

pub async fn create_staging_deployment(
    app_state: &AppState,
    input: CreateStagingDeploymentInput,
) -> Result<Deployment, AppError> {
    let command = CreateStagingDeploymentCommand::builder()
        .project_id(input.project_id)
        .auth_methods(input.auth_methods)
        .build()?;
    let mut tx = app_state.db_router.writer().begin().await?;
    let ids = AppStateIdGenerator::new(app_state);
    let deployment = command.execute_in_tx(&ids, &mut tx).await?;
    tx.commit().await?;
    Ok(deployment)
}

pub struct CreateProductionDeploymentInput {
    project_id: i64,
    custom_domain: String,
    auth_methods: Vec<String>,
}

impl CreateProductionDeploymentInput {
    pub fn new(project_id: i64, custom_domain: String, auth_methods: Vec<String>) -> Self {
        Self {
            project_id,
            custom_domain,
            auth_methods,
        }
    }
}

pub async fn create_production_deployment(
    app_state: &AppState,
    input: CreateProductionDeploymentInput,
) -> Result<Deployment, AppError> {
    let command = CreateProductionDeploymentCommand::builder()
        .project_id(input.project_id)
        .custom_domain(input.custom_domain)
        .auth_methods(input.auth_methods)
        .build()?;
    let mut tx = app_state.db_router.writer().begin().await?;
    let ids = AppStateIdGenerator::new(app_state);
    let deps = ProductionDeploymentDeps {
        ids: &ids,
        cloudflare_service: &app_state.cloudflare_service,
        postmark_service: &app_state.postmark_service,
    };
    let deployment = command.execute_in_tx(&deps, &mut tx).await?;
    tx.commit().await?;
    Ok(deployment)
}

pub struct VerifyDeploymentDnsRecordsInput {
    deployment_id: i64,
}

impl VerifyDeploymentDnsRecordsInput {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

pub async fn verify_deployment_dns_records(
    app_state: &AppState,
    input: VerifyDeploymentDnsRecordsInput,
) -> Result<Deployment, AppError> {
    let deps = VerifyDeploymentDnsDeps {
        cloudflare_service: &app_state.cloudflare_service,
        dns_verification_service: &app_state.dns_verification_service,
    };
    VerifyDeploymentDnsRecordsCommand::builder()
        .deployment_id(input.deployment_id)
        .build()?
        .execute_with(&deps, app_state.db_router.writer())
        .await
}

pub struct DeleteProjectInput {
    project_id: i64,
}

impl DeleteProjectInput {
    pub fn new(project_id: i64) -> Self {
        Self { project_id }
    }
}

pub async fn delete_project(app_state: &AppState, input: DeleteProjectInput) -> Result<(), AppError> {
    let command = DeleteProjectCommand::builder()
        .id(input.project_id)
        .build()?;
    let mut tx = app_state.db_router.writer().begin().await?;
    command.execute_in_tx(&mut tx).await?;
    tx.commit().await?;
    Ok(())
}
