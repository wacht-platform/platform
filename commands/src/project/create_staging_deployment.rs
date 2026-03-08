use super::*;
pub struct CreateStagingDeploymentCommand {
    project_id: i64,
    auth_methods: Vec<String>,
}

#[derive(Default)]
pub struct CreateStagingDeploymentCommandBuilder {
    project_id: Option<i64>,
    auth_methods: Option<Vec<String>>,
}

impl CreateStagingDeploymentCommand {
    pub fn builder() -> CreateStagingDeploymentCommandBuilder {
        CreateStagingDeploymentCommandBuilder::default()
    }

    pub fn new(project_id: i64, auth_methods: Vec<String>) -> Self {
        Self {
            project_id,
            auth_methods,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Deployment, AppError>
    where
        D: common::HasDbRouter + common::HasIdProvider + Sync,
    {
        let mut tx = deps.db_router().writer().begin().await?;

        let project = queries::ProjectWithBillingForStagingQuery::builder()
            .project_id(self.project_id)
            .execute_with_db(tx.as_mut())
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("Project with id {} not found", self.project_id))
            })?;

        ensure_billing_status_active(&project.status, "deployment")?;

        let deployment_row = create_staging_deployment_for_project(
            tx.as_mut(),
            deps,
            self.project_id,
            project.name.clone(),
            &self.auth_methods,
            project.pulse_usage_disabled,
        )
        .await?;

        tx.commit().await?;
        Ok(build_staging_deployment_model(deployment_row))
    }
}

impl CreateStagingDeploymentCommandBuilder {
    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub fn auth_methods(mut self, auth_methods: Vec<String>) -> Self {
        self.auth_methods = Some(auth_methods);
        self
    }

    pub fn build(self) -> Result<CreateStagingDeploymentCommand, AppError> {
        Ok(CreateStagingDeploymentCommand {
            project_id: self
                .project_id
                .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?,
            auth_methods: self
                .auth_methods
                .ok_or_else(|| AppError::Validation("auth_methods are required".to_string()))?,
        })
    }
}
