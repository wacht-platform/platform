use super::*;
pub struct CreateProductionDeploymentCommand {
    project_id: i64,
    custom_domain: String,
    auth_methods: Vec<String>,
}

#[derive(Default)]
pub struct CreateProductionDeploymentCommandBuilder {
    project_id: Option<i64>,
    custom_domain: Option<String>,
    auth_methods: Option<Vec<String>>,
}

impl CreateProductionDeploymentCommand {
    pub fn builder() -> CreateProductionDeploymentCommandBuilder {
        CreateProductionDeploymentCommandBuilder::default()
    }

    pub fn new(project_id: i64, custom_domain: String, auth_methods: Vec<String>) -> Self {
        Self {
            project_id,
            custom_domain,
            auth_methods,
        }
    }

    async fn cleanup_external_resources_on_failure(
        &self,
        deps: &ProductionDeploymentDeps<'_>,
        frontend_hostname: &str,
        backend_hostname: &str,
        domain: &str,
        postmark_domain_id: Option<i64>,
    ) {
        tracing::warn!("Cleaning up external resources for domain: {}", domain);

        if let Err(e) = deps
            .cloudflare_service
            .delete_custom_hostname(frontend_hostname)
            .await
        {
            tracing::error!(
                "Failed to cleanup frontend hostname {}: {}",
                frontend_hostname,
                e
            );
        } else {
            tracing::info!(
                "Successfully cleaned up frontend hostname: {}",
                frontend_hostname
            );
        }

        if let Err(e) = deps
            .cloudflare_service
            .delete_custom_hostname(backend_hostname)
            .await
        {
            tracing::error!(
                "Failed to cleanup backend hostname {}: {}",
                backend_hostname,
                e
            );
        } else {
            tracing::info!(
                "Successfully cleaned up backend hostname: {}",
                backend_hostname
            );
        }

        if let Some(domain_id) = postmark_domain_id {
            if let Err(e) = deps.postmark_service.delete_domain(domain_id).await {
                tracing::error!("Failed to cleanup Postmark domain {}: {}", domain_id, e);
            } else {
                tracing::info!("Successfully cleaned up Postmark domain: {}", domain_id);
            }
        } else {
            tracing::info!("No Postmark domain to cleanup for: {}", domain);
        }
    }

    pub async fn run_with_tx(
        self,
        deps: &ProductionDeploymentDeps<'_>,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Deployment, AppError> {
        let validator = ProjectValidator::new();
        validator.validate_domain_format(&self.custom_domain)?;
        validator.validate_auth_methods(&self.auth_methods)?;

        let requested_social_methods: Vec<&str> = self
            .auth_methods
            .iter()
            .map(String::as_str)
            .filter(|m| is_social_auth_method(m))
            .collect();

        if !requested_social_methods.is_empty() {
            return Err(AppError::Validation(
                "Social authentication cannot be enabled during production deployment creation. Configure social providers later with custom credentials in deployment settings.".to_string(),
            ));
        }

        let (public_key, private_key, saml_public_key, saml_private_key) =
            generate_deployment_key_pairs().await?;

        let project = ProjectForProductionQuery::builder()
            .project_id(self.project_id)
            .execute_with(tx.as_mut())
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

        if ExistingProductionDeploymentQuery::builder()
            .project_id(self.project_id)
            .execute_with(tx.as_mut())
            .await?
            .is_some()
        {
            return Err(AppError::BadRequest(
                "A production deployment already exists for this project".to_string(),
            ));
        }

        if let Some(existing) = ExistingDomainDeploymentQuery::builder()
            .custom_domain(&self.custom_domain)
            .execute_with(tx.as_mut())
            .await?
        {
            return Err(AppError::BadRequest(format!(
                "Domain '{}' is already in use by another deployment (ID: {})",
                self.custom_domain, existing.id
            )));
        }

        let backend_host = format!("frontend.{}", self.custom_domain);
        let frontend_host = format!("accounts.{}", self.custom_domain);
        let mail_from_host = format!("wcmail.{}", self.custom_domain);

        let domain_verification_records = deps
            .cloudflare_service
            .generate_domain_verification_records(&frontend_host, &backend_host);

        let empty_email_verification_records = EmailVerificationRecords::default();

        let mut publishable_key = String::from("pk_live_");
        let base64_backend_host = BASE64_STANDARD.encode(format!("https://{}", backend_host));
        publishable_key.push_str(&base64_backend_host);

        let deployment_row = ProductionDeploymentInsert::builder()
            .id(deps.ids.next_id()?)
            .project_id(self.project_id)
            .backend_host(backend_host)
            .frontend_host(frontend_host.clone())
            .publishable_key(publishable_key)
            .mail_from_host(mail_from_host.clone())
            .domain_verification_records(
                serde_json::to_value(&domain_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .email_verification_records(
                serde_json::to_value(&empty_email_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .execute_with(tx.as_mut())
            .await?;

        let auth_settings = build_auth_settings(&self.auth_methods, deployment_row.id);
        DeploymentAuthSettingsInsert::builder()
            .id(deps.ids.next_id()?)
            .auth_settings(auth_settings)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let ui_settings =
            build_ui_settings(deployment_row.id, &frontend_host, project.name.clone());
        let b2b_settings = build_b2b_settings(deployment_row.id);
        let restrictions = build_restrictions(deployment_row.id);
        let email_templates = build_email_templates(deployment_row.id);
        let sms_templates = build_sms_templates(deployment_row.id);
        DeploymentKeyPairsInsert::builder()
            .id(deps.ids.next_id()?)
            .deployment_id(deployment_row.id)
            .public_key(public_key)
            .private_key(private_key)
            .saml_public_key(saml_public_key)
            .saml_private_key(saml_private_key)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let waitlist_url = format!("{}/waitlist", frontend_host);
        DeploymentUiSettingsInsert::builder()
            .id(deps.ids.next_id()?)
            .ui_settings(ui_settings)
            .waitlist_page_url(waitlist_url)
            .support_page_url("")
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        DeploymentB2bBootstrapInsert::builder()
            .settings_row_id(deps.ids.next_id()?)
            .workspace_creator_role_id(deps.ids.next_id()?)
            .workspace_member_role_id(deps.ids.next_id()?)
            .org_creator_role_id(deps.ids.next_id()?)
            .org_member_role_id(deps.ids.next_id()?)
            .b2b_settings(b2b_settings)
            .build()?
            .execute_with_deps(tx.as_mut())
            .await?;

        DeploymentRestrictionsInsert::builder()
            .id(deps.ids.next_id()?)
            .restrictions(restrictions)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        DeploymentEmailTemplatesInsert::builder()
            .id(deps.ids.next_id()?)
            .email_templates(email_templates)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        DeploymentSmsTemplatesInsert::builder()
            .id(deps.ids.next_id()?)
            .sms_templates(sms_templates)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        DeploymentAiSettingsInsert::builder()
            .id(deps.ids.next_id()?)
            .deployment_id(deployment_row.id)
            .build()?
            .execute_with(tx.as_mut())
            .await?;

        let postmark_domain = deps.postmark_service.create_domain(&mail_from_host).await?;
        let postmark_domain_id = postmark_domain.id;
        let email_verification_records = deps
            .postmark_service
            .generate_email_verification_records(&postmark_domain);

        DeploymentEmailVerificationUpdate::builder()
            .deployment_id(deployment_row.id)
            .email_verification_records(
                serde_json::to_value(&email_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .execute_with(tx.as_mut())
            .await?;

        let frontend_hostname = format!("accounts.{}", self.custom_domain);
        let backend_hostname = format!("frontend.{}", self.custom_domain);

        let frontend_hostname_result = deps
            .cloudflare_service
            .create_custom_hostname(&frontend_hostname, "accounts.wacht.services")
            .await;

        let frontend_hostname_id = match frontend_hostname_result {
            Ok(custom_hostname) => {
                tracing::info!(
                    "Successfully created frontend custom hostname: {}",
                    frontend_hostname
                );
                Some(custom_hostname.id)
            }
            Err(e) => {
                tracing::error!("Failed to create frontend custom hostname: {}", e);

                return Err(AppError::External(format!(
                    "Failed to create frontend custom hostname: {}. Deployment has been cleaned up.",
                    e
                )));
            }
        };

        let backend_hostname_result = deps
            .cloudflare_service
            .create_custom_hostname(&backend_hostname, "frontend.wacht.services")
            .await;

        let backend_hostname_id = match backend_hostname_result {
            Ok(custom_hostname) => {
                tracing::info!(
                    "Successfully created backend custom hostname: {}",
                    backend_hostname
                );
                Some(custom_hostname.id)
            }
            Err(e) => {
                tracing::error!("Failed to create backend custom hostname: {}", e);
                self.cleanup_external_resources_on_failure(
                    deps,
                    &frontend_hostname,
                    &backend_hostname,
                    &self.custom_domain,
                    Some(postmark_domain_id),
                )
                .await;

                return Err(AppError::External(format!(
                    "Failed to create backend custom hostname: {}. Resources have been cleaned up.",
                    e
                )));
            }
        };

        tracing::info!(
            "Postmark domain created successfully for: {}",
            self.custom_domain
        );

        let mut updated_domain_verification_records = domain_verification_records;
        updated_domain_verification_records.frontend_hostname_id = frontend_hostname_id;
        updated_domain_verification_records.backend_hostname_id = backend_hostname_id;
        DeploymentDomainVerificationUpdate::builder()
            .deployment_id(deployment_row.id)
            .domain_verification_records(
                serde_json::to_value(&updated_domain_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .execute_with(tx.as_mut())
            .await?;

        let console_id = console_deployment_id()?;

        ConsoleAppBootstrapInsert::builder()
            .console_deployment_id(console_id)
            .target_deployment_id(deployment_row.id)
            .event_catalog_slug(DEFAULT_WEBHOOK_EVENT_CATALOG_SLUG)
            .build()?
            .execute_with_deps(tx.as_mut())
            .await?;

        tracing::info!(
            "Successfully created production deployment for domain: {} with hostnames: {}, {}",
            self.custom_domain,
            frontend_hostname,
            backend_hostname
        );

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
            domain_verification_records: Some(updated_domain_verification_records),
            email_verification_records: Some(email_verification_records),
            email_provider: EmailProvider::from(deployment_row.email_provider),
            custom_smtp_config: deployment_row
                .custom_smtp_config
                .and_then(|v| serde_json::from_value(v).ok())
                .map(|mut c: CustomSmtpConfig| {
                    c.password = String::new();
                    c
                }),
        })
    }

    pub async fn execute_with(
        self,
        writer: &sqlx::PgPool,
        deps: &ProductionDeploymentDeps<'_>,
    ) -> Result<Deployment, AppError> {
        let mut tx = writer.begin().await?;
        let result = self.run_with_tx(deps, &mut tx).await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Deployment, AppError>
    where
        D: common::HasDbRouter
            + common::HasIdGenerator
            + common::HasCloudflareService
            + common::HasPostmarkService
            + Sync,
    {
        let mut tx = deps.db_router().writer().begin().await?;
        let ids = DepsIdGeneratorAdapter::new(deps);
        let production_deps = ProductionDeploymentDeps {
            ids: &ids,
            cloudflare_service: deps.cloudflare_service(),
            postmark_service: deps.postmark_service(),
        };
        let result = self.run_with_tx(&production_deps, &mut tx).await?;
        tx.commit().await?;
        Ok(result)
    }
}

impl CreateProductionDeploymentCommandBuilder {
    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub fn custom_domain(mut self, custom_domain: impl Into<String>) -> Self {
        self.custom_domain = Some(custom_domain.into());
        self
    }

    pub fn auth_methods(mut self, auth_methods: Vec<String>) -> Self {
        self.auth_methods = Some(auth_methods);
        self
    }

    pub fn build(self) -> Result<CreateProductionDeploymentCommand, AppError> {
        Ok(CreateProductionDeploymentCommand {
            project_id: self
                .project_id
                .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?,
            custom_domain: self
                .custom_domain
                .ok_or_else(|| AppError::Validation("custom_domain is required".to_string()))?,
            auth_methods: self
                .auth_methods
                .ok_or_else(|| AppError::Validation("auth_methods are required".to_string()))?,
        })
    }
}
