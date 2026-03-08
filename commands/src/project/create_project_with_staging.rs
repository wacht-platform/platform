use super::*;
pub struct CreateProjectWithStagingDeploymentCommand {
    name: String,
    auth_methods: Vec<String>,
    owner_id: Option<String>,
}

#[derive(Default)]
pub struct CreateProjectWithStagingDeploymentCommandBuilder {
    name: Option<String>,
    auth_methods: Option<Vec<String>>,
    owner_id: Option<String>,
}

impl CreateProjectWithStagingDeploymentCommand {
    pub fn builder() -> CreateProjectWithStagingDeploymentCommandBuilder {
        CreateProjectWithStagingDeploymentCommandBuilder::default()
    }

    pub fn new(name: String, auth_methods: Vec<String>) -> Self {
        Self {
            name,
            auth_methods,
            owner_id: None,
        }
    }

    pub fn with_owner(mut self, owner_id: String) -> Self {
        self.owner_id = Some(owner_id);
        self
    }

    fn owner_id_fragment(owner_id: &str) -> Result<&str, AppError> {
        owner_id
            .split('_')
            .next_back()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Validation("Invalid owner id format".to_string()))
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<ProjectWithDeployments, AppError>
    where
        D: common::HasDbRouter + common::HasIdGenerator + Sync,
    {
        let mut tx = deps.db_router().writer().begin().await?;
        ProjectValidator::validate_project_name(&self.name)?;

        let project_id = next_id_from(deps)?;

        let owner_id = self
            .owner_id
            .as_deref()
            .ok_or_else(|| AppError::Validation("Project must have an owner".to_string()))?;
        let owner_id_fragment = Self::owner_id_fragment(owner_id)?;
        let billing_account = queries::BillingAccountForOwnerLockQuery::builder()
            .owner_id(owner_id)
            .execute_with_db(tx.as_mut())
            .await?
            .ok_or_else(|| AppError::Validation("No billing account found".to_string()))?;

        ensure_billing_status_active(&billing_account.status, "project")?;

        let billing_account_id = billing_account.id;
        let project_count = queries::ProjectsCountByBillingAccountQuery::builder()
            .billing_account_id(billing_account_id)
            .execute_with_db(tx.as_mut())
            .await?;

        if project_count >= MAX_PROJECTS_PER_BILLING_ACCOUNT {
            return Err(AppError::Validation(format!(
                "Project limit reached. You can create up to {} projects.",
                MAX_PROJECTS_PER_BILLING_ACCOUNT
            )));
        }

        let project_row = ProjectInsert::builder()
            .id(project_id)
            .name(self.name.clone())
            .owner_id_fragment(owner_id_fragment)
            .billing_account_id(billing_account_id)
            .execute_with_db(tx.as_mut())
            .await?;

        let deployment_row = create_staging_deployment_for_project(
            tx.as_mut(),
            deps,
            project_row.id,
            self.name.clone(),
            &self.auth_methods,
            billing_account.pulse_usage_disabled,
        )
        .await?;

        tx.commit().await?;
        Ok(ProjectWithDeployments {
            id: project_row.id,
            image_url: project_row.image_url,
            created_at: project_row.created_at,
            updated_at: project_row.updated_at,
            name: project_row.name,
            owner_id: project_row.owner_id,
            billing_account_id,
            deployments: vec![build_staging_deployment_model(deployment_row)],
        })
    }
}

impl CreateProjectWithStagingDeploymentCommandBuilder {
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn auth_methods(mut self, auth_methods: Vec<String>) -> Self {
        self.auth_methods = Some(auth_methods);
        self
    }

    pub fn owner_id(mut self, owner_id: impl Into<String>) -> Self {
        self.owner_id = Some(owner_id.into());
        self
    }

    pub fn build(self) -> Result<CreateProjectWithStagingDeploymentCommand, AppError> {
        Ok(CreateProjectWithStagingDeploymentCommand {
            name: self
                .name
                .ok_or_else(|| AppError::Validation("name is required".to_string()))?,
            auth_methods: self
                .auth_methods
                .ok_or_else(|| AppError::Validation("auth_methods are required".to_string()))?,
            owner_id: self.owner_id,
        })
    }
}
