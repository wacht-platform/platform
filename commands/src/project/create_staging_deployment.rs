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

    fn create_auth_settings(&self, deployment_id: i64) -> DeploymentAuthSettings {
        build_auth_settings(&self.auth_methods, deployment_id)
    }

    fn create_b2b_settings(&self, deployment_id: i64) -> DeploymentB2bSettingsWithRoles {
        build_b2b_settings(deployment_id)
    }

    fn create_ui_settings(
        &self,
        deployment_id: i64,
        frontend_host: String,
        app_name: String,
    ) -> DeploymentUISettings {
        build_ui_settings(deployment_id, &frontend_host, app_name)
    }

    fn create_restrictions(&self, deployment_id: i64) -> DeploymentRestrictions {
        build_restrictions(deployment_id)
    }

    fn create_sms_templates(&self, deployment_id: i64) -> DeploymentSmsTemplate {
        build_sms_templates(deployment_id)
    }

    fn create_email_templates(&self, deployment_id: i64) -> DeploymentEmailTemplate {
        build_email_templates(deployment_id)
    }

    pub async fn execute_in_tx(
        self,
        ids: &dyn IdGenerator,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Deployment, AppError> {
        let validator = ProjectValidator::new();
        validator.validate_auth_methods(&self.auth_methods)?;

        let (public_key, private_key, saml_public_key, saml_private_key) =
            generate_deployment_key_pairs().await?;

        let project = ProjectWithBillingForStagingQuery::builder()
            .project_id(self.project_id)
            .execute_in_tx(tx)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("Project with id {} not found", self.project_id))
            })?;

        if project.status != "active" {
            return Err(AppError::Validation(format!(
                "Cannot create deployment. Billing account status is {}",
                project.status
            )));
        }

        if includes_phone_auth(&self.auth_methods) && project.pulse_usage_disabled {
            return Err(AppError::Validation(
                "Prepaid recharge is required before enabling phone authentication for staging deployments".to_string(),
            ));
        }

        let staging_count = StagingDeploymentCountByProjectQuery::builder()
            .project_id(self.project_id)
            .execute_in_tx(tx)
            .await?;

        if staging_count >= 3 {
            return Err(AppError::BadRequest(
                "Maximum of 3 staging deployments allowed per project".to_string(),
            ));
        }

        let hostname = generate_nanoid();
        let backend_host = format!("{}.fapi.trywacht.xyz", hostname);
        let frontend_host = format!("{}.accounts.trywacht.xyz", hostname);

        let mut publishable_key = String::from("pk_test_");
        let base64_backend_host = BASE64_STANDARD.encode(format!("https://{}", backend_host));
        publishable_key.push_str(&base64_backend_host);

        let deployment_row = StagingDeploymentInsert::builder()
            .id(ids.next_id()?)
            .project_id(self.project_id)
            .backend_host(backend_host)
            .frontend_host(frontend_host)
            .publishable_key(publishable_key)
            .mail_from_host("staging.wacht.services")
            .execute_in_tx(tx)
            .await?;

        let auth_settings = self.create_auth_settings(deployment_row.id);
        DeploymentAuthSettingsInsert::builder()
            .id(ids.next_id()?)
            .auth_settings(auth_settings)
            .build()?
            .execute_in_tx(tx)
            .await?;

        let ui_settings = self.create_ui_settings(
            deployment_row.id,
            deployment_row.frontend_host.clone(),
            project.name.clone(),
        );
        let waitlist_url = format!("https://{}/waitlist", deployment_row.frontend_host);
        DeploymentUiSettingsInsert::builder()
            .id(ids.next_id()?)
            .ui_settings(ui_settings)
            .waitlist_page_url(waitlist_url)
            .support_page_url("")
            .build()?
            .execute_in_tx(tx)
            .await?;

        let b2b_settings = self.create_b2b_settings(deployment_row.id);

        DeploymentB2bBootstrapInsert::builder()
            .settings_row_id(ids.next_id()?)
            .workspace_creator_role_id(ids.next_id()?)
            .workspace_member_role_id(ids.next_id()?)
            .org_creator_role_id(ids.next_id()?)
            .org_member_role_id(ids.next_id()?)
            .b2b_settings(b2b_settings)
            .build()?
            .execute_in_tx(tx)
            .await?;

        let restrictions = self.create_restrictions(deployment_row.id);

        DeploymentRestrictionsInsert::builder()
            .id(ids.next_id()?)
            .restrictions(restrictions)
            .build()?
            .execute_in_tx(tx)
            .await?;

        let sms_templates = self.create_sms_templates(deployment_row.id);

        DeploymentSmsTemplatesInsert::builder()
            .id(ids.next_id()?)
            .sms_templates(sms_templates)
            .build()?
            .execute_in_tx(tx)
            .await?;

        DeploymentAiSettingsInsert::builder()
            .id(ids.next_id()?)
            .deployment_id(deployment_row.id)
            .build()?
            .execute_in_tx(tx)
            .await?;

        DeploymentKeyPairsInsert::builder()
            .id(ids.next_id()?)
            .deployment_id(deployment_row.id)
            .public_key(public_key)
            .private_key(private_key)
            .saml_public_key(saml_public_key)
            .saml_private_key(saml_private_key)
            .build()?
            .execute_in_tx(tx)
            .await?;

        let email_templates = self.create_email_templates(deployment_row.id);

        DeploymentEmailTemplatesInsert::builder()
            .id(ids.next_id()?)
            .email_templates(email_templates)
            .build()?
            .execute_in_tx(tx)
            .await?;

        if let Some(social_connections_insert) =
            DeploymentSocialConnectionsBulkInsert::from_auth_methods(
                deployment_row.id,
                &self.auth_methods,
                || ids.next_id(),
            )?
        {
            social_connections_insert.execute_in_tx(tx).await?;
        }

        let console_id = console_deployment_id()?;

        ConsoleAppBootstrapInsert::builder()
            .console_deployment_id(console_id)
            .target_deployment_id(deployment_row.id)
            .event_catalog_slug(DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG)
            .build()?
            .execute_in_tx(tx)
            .await?;

        Ok(Deployment {
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
        })
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

impl Command for CreateStagingDeploymentCommand {
    type Output = Deployment;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_router.writer().begin().await?;
        let ids = AppStateIdGenerator::new(app_state);
        let result = self.execute_in_tx(&ids, &mut tx).await?;
        tx.commit().await?;
        Ok(result)
    }
}
