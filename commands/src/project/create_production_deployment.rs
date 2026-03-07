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

    pub async fn execute_with_deps<D>(self, app_deps: &D) -> Result<Deployment, AppError>
    where
        D: common::HasDbRouter
            + common::HasIdGenerator
            + common::HasCloudflareService
            + common::HasPostmarkService
            + Sync,
    {
        let mut tx = app_deps.db_router().writer().begin().await?;
        let ids = DepsIdGeneratorAdapter::new(app_deps);
        let deps = ProductionDeploymentDeps {
            ids: &ids,
            cloudflare_service: app_deps.cloudflare_service(),
            postmark_service: app_deps.postmark_service(),
        };
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

        let key_material = generate_deployment_key_material().await?;

        let project = ProjectForProductionQuery::builder()
            .project_id(self.project_id)
            .execute_with_db(tx.as_mut())
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("Project with id {} not found", self.project_id))
            })?;

        ensure_billing_status_active(&project.status, "deployment")?;

        if ExistingProductionDeploymentQuery::builder()
            .project_id(self.project_id)
            .execute_with_db(tx.as_mut())
            .await?
            .is_some()
        {
            return Err(AppError::BadRequest(
                "A production deployment already exists for this project".to_string(),
            ));
        }

        if let Some(existing) = ExistingDomainDeploymentQuery::builder()
            .custom_domain(&self.custom_domain)
            .execute_with_db(tx.as_mut())
            .await?
        {
            return Err(AppError::BadRequest(format!(
                "Domain '{}' is already in use by another deployment (ID: {})",
                self.custom_domain, existing.id
            )));
        }

        let hosts = build_production_deployment_hosts(&self.custom_domain);

        let domain_verification_records = deps
            .cloudflare_service
            .generate_domain_verification_records(&hosts.frontend_host, &hosts.backend_host);

        let empty_email_verification_records = EmailVerificationRecords::default();

        let deployment_row = ProductionDeploymentInsert::builder()
            .id(deps.ids.next_id()?)
            .project_id(self.project_id)
            .backend_host(hosts.backend_host.clone())
            .frontend_host(hosts.frontend_host.clone())
            .publishable_key(hosts.publishable_key)
            .mail_from_host(hosts.mail_from_host.clone())
            .domain_verification_records(
                serde_json::to_value(&domain_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .email_verification_records(
                serde_json::to_value(&empty_email_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .execute_with_db(tx.as_mut())
            .await?;

        let waitlist_url = format!("{}/waitlist", hosts.frontend_host);
        bootstrap_deployment_defaults(
            tx.as_mut(),
            deps.ids,
            DeploymentBootstrapInput {
                deployment_id: deployment_row.id,
                frontend_host: &hosts.frontend_host,
                app_name: project.name.clone(),
                auth_methods: &self.auth_methods,
                waitlist_page_url: waitlist_url,
                support_page_url: "",
                key_material,
            },
        )
        .await?;

        let postmark_domain = deps
            .postmark_service
            .create_domain(&hosts.mail_from_host)
            .await?;
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
            .execute_with_db(tx.as_mut())
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
                    &deps,
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
            .execute_with_db(tx.as_mut())
            .await?;

        tracing::info!(
            "Successfully created production deployment for domain: {} with hostnames: {}, {}",
            self.custom_domain,
            frontend_hostname,
            backend_hostname
        );

        tx.commit().await?;
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
