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
        D: common::HasDbRouter + common::HasIdGenerator + Sync,
    {
        let mut tx = deps.db_router().writer().begin().await?;
        let ids = DepsIdGeneratorAdapter::new(deps);
        let validator = ProjectValidator::new();
        validator.validate_auth_methods(&self.auth_methods)?;

        let key_material = generate_deployment_key_material().await?;

        let project = ProjectWithBillingForStagingQuery::builder()
            .project_id(self.project_id)
            .execute_with_db(tx.as_mut())
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("Project with id {} not found", self.project_id))
            })?;

        ensure_billing_status_active(&project.status, "deployment")?;
        ensure_phone_auth_allowed(&self.auth_methods, project.pulse_usage_disabled)?;

        let staging_count = StagingDeploymentCountByProjectQuery::builder()
            .project_id(self.project_id)
            .execute_with_db(tx.as_mut())
            .await?;

        if staging_count >= 3 {
            return Err(AppError::BadRequest(
                "Maximum of 3 staging deployments allowed per project".to_string(),
            ));
        }

        let deployment_row = insert_staging_deployment_with_defaults(
            tx.as_mut(),
            &ids,
            self.project_id,
            project.name.clone(),
            &self.auth_methods,
            key_material,
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
