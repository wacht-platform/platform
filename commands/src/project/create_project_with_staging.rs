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

    fn owner_id_fragment(&self) -> Result<&str, AppError> {
        let owner_id = self
            .owner_id
            .as_deref()
            .ok_or_else(|| AppError::Validation("Project must have an owner".to_string()))?;

        owner_id
            .split('_')
            .next_back()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Validation("Invalid owner id format".to_string()))
    }

    pub async fn run_with_tx(
        self,
        ids: &dyn IdGenerator,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<ProjectWithDeployments, AppError> {
        let validator = ProjectValidator::new();
        validator.validate_project_name(&self.name)?;
        validator.validate_auth_methods(&self.auth_methods)?;

        let project_id = ids.next_id()?;

        let (public_key, private_key, saml_public_key, saml_private_key) =
            generate_deployment_key_pairs().await?;

        let owner_id = self
            .owner_id
            .as_deref()
            .ok_or_else(|| AppError::Validation("Project must have an owner".to_string()))?;
        let billing_account = BillingAccountForOwnerLockQuery::builder()
            .owner_id(owner_id)
            .execute_with(tx.as_mut())
            .await?
            .ok_or_else(|| AppError::Validation("No billing account found".to_string()))?;

        if billing_account.status != "active" {
            return Err(AppError::Validation(format!(
                "Cannot create project. Billing account status is {}",
                billing_account.status
            )));
        }

        if includes_phone_auth(&self.auth_methods) && billing_account.pulse_usage_disabled {
            return Err(AppError::Validation(
                "Prepaid recharge is required before enabling phone authentication for staging deployments".to_string(),
            ));
        }

        let billing_account_id = billing_account.id;
        let project_count = ProjectsCountByBillingAccountQuery::builder()
            .billing_account_id(billing_account_id)
            .execute_with(tx.as_mut())
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
            .owner_id_fragment(self.owner_id_fragment()?)
            .billing_account_id(billing_account_id)
            .execute_with(tx.as_mut())
            .await?;

        let hostname = generate_nanoid();

        let backend_host = format!("{}.fapi.trywacht.xyz", hostname);
        let frontend_host = format!("{}.accounts.trywacht.xyz", hostname);
        let mut publishable_key = String::from("pk_test_");

        let base64_backend_host = BASE64_STANDARD.encode(format!("https://{}", backend_host));
        publishable_key.push_str(&base64_backend_host);

        let deployment_row = StagingDeploymentInsert::builder()
            .id(ids.next_id()?)
            .project_id(project_row.id)
            .backend_host(backend_host)
            .frontend_host(frontend_host)
            .publishable_key(publishable_key)
            .mail_from_host("staging.wacht.services")
            .execute_with(tx.as_mut())
            .await?;

        let auth_settings = build_auth_settings(&self.auth_methods, deployment_row.id);
        DeploymentAuthSettingsInsert::builder()
            .id(ids.next_id()?)
            .auth_settings(auth_settings)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let ui_settings = build_ui_settings(
            deployment_row.id,
            &format!("{}.accounts.trywacht.xyz", hostname),
            self.name.clone(),
        );
        let waitlist_url = format!("https://{}.accounts.trywacht.xyz/waitlist", hostname);
        DeploymentUiSettingsInsert::builder()
            .id(ids.next_id()?)
            .ui_settings(ui_settings)
            .waitlist_page_url(waitlist_url)
            .support_page_url("")
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let restrictions = build_restrictions(deployment_row.id);

        DeploymentRestrictionsInsert::builder()
            .id(ids.next_id()?)
            .restrictions(restrictions)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let b2b_settings = build_b2b_settings(deployment_row.id);

        let sms_templates = build_sms_templates(deployment_row.id);

        DeploymentSmsTemplatesInsert::builder()
            .id(ids.next_id()?)
            .sms_templates(sms_templates)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        DeploymentKeyPairsInsert::builder()
            .id(ids.next_id()?)
            .deployment_id(deployment_row.id)
            .public_key(public_key)
            .private_key(private_key)
            .saml_public_key(saml_public_key)
            .saml_private_key(saml_private_key)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let email_templates = build_email_templates(deployment_row.id);

        DeploymentEmailTemplatesInsert::builder()
            .id(ids.next_id()?)
            .email_templates(email_templates)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        DeploymentB2bBootstrapInsert::builder()
            .settings_row_id(ids.next_id()?)
            .workspace_creator_role_id(ids.next_id()?)
            .workspace_member_role_id(ids.next_id()?)
            .org_creator_role_id(ids.next_id()?)
            .org_member_role_id(ids.next_id()?)
            .b2b_settings(b2b_settings)
            .build()?
            .execute_with_deps(tx.as_mut())
            .await?;

        if let Some(social_connections_insert) =
            DeploymentSocialConnectionsBulkInsert::from_auth_methods(
                deployment_row.id,
                &self.auth_methods,
                || ids.next_id(),
            )?
        {
            social_connections_insert.execute_with(tx.as_mut()).await?;
        }

        let console_id = console_deployment_id()?;

        ConsoleAppBootstrapInsert::builder()
            .console_deployment_id(console_id)
            .target_deployment_id(deployment_row.id)
            .event_catalog_slug(DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG)
            .build()?
            .execute_with_deps(tx.as_mut())
            .await?;

        DeploymentAiSettingsInsert::builder()
            .id(ids.next_id()?)
            .deployment_id(deployment_row.id)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let deployment = Deployment {
            id: deployment_row.id,
            created_at: deployment_row.created_at,
            updated_at: deployment_row.updated_at,
            maintenance_mode: deployment_row.maintenance_mode,
            backend_host: deployment_row.backend_host,
            frontend_host: deployment_row.frontend_host,
            publishable_key: deployment_row.publishable_key,
            project_id: deployment_row.project_id,
            mode: DeploymentMode::from(deployment_row.mode),
            mail_from_host: deployment_row.mail_from_host,
            domain_verification_records: None,
            email_verification_records: None,
            email_provider: EmailProvider::default(),
            custom_smtp_config: None,
        };

        Ok(ProjectWithDeployments {
            id: project_row.id,
            image_url: project_row.image_url,
            created_at: project_row.created_at,
            updated_at: project_row.updated_at,
            name: project_row.name,
            owner_id: project_row.owner_id,
            billing_account_id,
            deployments: vec![deployment],
        })
    }

    pub async fn execute_with(
        self,
        writer: &sqlx::PgPool,
        ids: &dyn IdGenerator,
    ) -> Result<ProjectWithDeployments, AppError> {
        let mut tx = writer.begin().await?;
        let result = self.run_with_tx(ids, &mut tx).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<ProjectWithDeployments, AppError>
    where
        D: common::HasDbRouter + common::HasIdGenerator + Sync,
    {
        let mut tx = deps.db_router().writer().begin().await?;
        let ids = DepsIdGeneratorAdapter::new(deps);
        let result = self.run_with_tx(&ids, &mut tx).await?;
        tx.commit().await?;
        Ok(result)
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
