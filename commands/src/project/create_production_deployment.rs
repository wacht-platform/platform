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
        cloudflare_service: &common::CloudflareService,
        postmark_service: &common::PostmarkService,
        frontend_hostname: &str,
        backend_hostname: &str,
        domain: &str,
        postmark_domain_id: Option<i64>,
    ) {
        tracing::warn!("Cleaning up external resources for domain: {}", domain);

        if let Err(e) = cloudflare_service
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

        if let Err(e) = cloudflare_service
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
            if let Err(e) = postmark_service.delete_domain(domain_id).await {
                tracing::error!("Failed to cleanup Postmark domain {}: {}", domain_id, e);
            } else {
                tracing::info!("Successfully cleaned up Postmark domain: {}", domain_id);
            }
        } else {
            tracing::info!("No Postmark domain to cleanup for: {}", domain);
        }
    }

    fn ensure_no_social_methods_requested(&self) -> Result<(), AppError> {
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

        Ok(())
    }

    async fn load_project_for_production(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<queries::ProjectForProductionRow, AppError> {
        queries::ProjectForProductionQuery::builder()
            .project_id(self.project_id)
            .execute_with_db(&mut *conn)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("Project with id {} not found", self.project_id))
            })
    }

    async fn ensure_production_deployment_is_unique(
        &self,
        conn: &mut sqlx::PgConnection,
    ) -> Result<(), AppError> {
        if queries::ExistingProductionDeploymentQuery::builder()
            .project_id(self.project_id)
            .execute_with_db(&mut *conn)
            .await?
            .is_some()
        {
            return Err(AppError::BadRequest(
                "A production deployment already exists for this project".to_string(),
            ));
        }

        if let Some(existing) = queries::ExistingDomainDeploymentQuery::builder()
            .custom_domain(&self.custom_domain)
            .execute_with_db(&mut *conn)
            .await?
        {
            return Err(AppError::BadRequest(format!(
                "Domain '{}' is already in use by another deployment (ID: {})",
                self.custom_domain, existing.id
            )));
        }

        Ok(())
    }

    async fn create_custom_hostname<D>(
        &self,
        deps: &D,
        hostname: &str,
        target: &str,
        kind: &str,
    ) -> Result<String, AppError>
    where
        D: common::HasCloudflareProvider,
    {
        match deps
            .cloudflare_provider()
            .create_custom_hostname(hostname, target)
            .await
        {
            Ok(custom_hostname) => {
                tracing::info!(
                    "Successfully created {} custom hostname: {}",
                    kind,
                    hostname
                );
                Ok(custom_hostname.id)
            }
            Err(e) => {
                tracing::error!("Failed to create {} custom hostname: {}", kind, e);
                Err(AppError::External(format!(
                    "Failed to create {} custom hostname: {}",
                    kind, e
                )))
            }
        }
    }

    async fn provision_custom_hostnames<D>(
        &self,
        deps: &D,
        frontend_hostname: &str,
        backend_hostname: &str,
        postmark_domain_id: i64,
    ) -> Result<(Option<String>, Option<String>), AppError>
    where
        D: common::HasCloudflareProvider + common::HasPostmarkProvider,
    {
        let frontend_hostname_id = self
            .create_custom_hostname(
                deps,
                frontend_hostname,
                "accounts.wacht.services",
                "frontend",
            )
            .await
            .map(Some)
            .map_err(|e| {
                AppError::External(format!(
                    "{}. Deployment has been cleaned up.",
                    e.to_string()
                ))
            })?;

        let backend_hostname_id = match self
            .create_custom_hostname(deps, backend_hostname, "frontend.wacht.services", "backend")
            .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                self.cleanup_external_resources_on_failure(
                    deps.cloudflare_provider(),
                    deps.postmark_provider(),
                    frontend_hostname,
                    backend_hostname,
                    &self.custom_domain,
                    Some(postmark_domain_id),
                )
                .await;

                return Err(AppError::External(format!(
                    "{}. Resources have been cleaned up.",
                    e
                )));
            }
        };

        Ok((frontend_hostname_id, backend_hostname_id))
    }

    async fn setup_postmark_email_verification<D>(
        &self,
        deps: &D,
        conn: &mut sqlx::PgConnection,
        deployment_id: i64,
        mail_from_host: &str,
    ) -> Result<(i64, EmailVerificationRecords), AppError>
    where
        D: common::HasPostmarkProvider,
    {
        let postmark_domain = deps
            .postmark_provider()
            .create_domain(mail_from_host)
            .await?;
        let postmark_domain_id = postmark_domain.id;
        let email_verification_records = deps
            .postmark_provider()
            .generate_email_verification_records(&postmark_domain);

        DeploymentEmailVerificationUpdate::builder()
            .deployment_id(deployment_id)
            .email_verification_records(json_value(&email_verification_records)?)
            .execute_with_db(conn)
            .await?;

        Ok((postmark_domain_id, email_verification_records))
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Deployment, AppError>
    where
        D: common::HasDbRouter
            + common::HasIdProvider
            + common::HasCloudflareProvider
            + common::HasPostmarkProvider
            + Sync,
    {
        let mut tx = deps.db_router().writer().begin().await?;
        ProjectValidator::validate_domain_format(&self.custom_domain)?;
        ProjectValidator::validate_auth_methods(&self.auth_methods)?;
        self.ensure_no_social_methods_requested()?;

        let key_material = generate_deployment_key_material().await?;
        let project = self.load_project_for_production(tx.as_mut()).await?;

        ensure_billing_status_active(&project.status, "deployment")?;
        self.ensure_production_deployment_is_unique(tx.as_mut())
            .await?;

        let hosts = build_production_deployment_hosts(&self.custom_domain);
        let frontend_hostname = hosts.frontend_host.clone();
        let backend_hostname = hosts.backend_host.clone();

        let domain_verification_records = deps
            .cloudflare_provider()
            .generate_domain_verification_records(&frontend_hostname, &backend_hostname);

        let empty_email_verification_records = EmailVerificationRecords::default();

        let deployment_row = ProductionDeploymentInsert::builder()
            .id(next_id_from(deps)?)
            .project_id(self.project_id)
            .backend_host(hosts.backend_host.clone())
            .frontend_host(hosts.frontend_host.clone())
            .publishable_key(hosts.publishable_key)
            .mail_from_host(hosts.mail_from_host.clone())
            .domain_verification_records(json_value(&domain_verification_records)?)
            .email_verification_records(json_value(&empty_email_verification_records)?)
            .execute_with_db(tx.as_mut())
            .await?;

        let waitlist_url = format!("https://{}/waitlist", hosts.frontend_host);
        bootstrap_deployment_defaults(
            tx.as_mut(),
            deps,
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

        let (postmark_domain_id, email_verification_records) = self
            .setup_postmark_email_verification(
                deps,
                tx.as_mut(),
                deployment_row.id,
                &hosts.mail_from_host,
            )
            .await?;

        let (frontend_hostname_id, backend_hostname_id) = self
            .provision_custom_hostnames(
                deps,
                &frontend_hostname,
                &backend_hostname,
                postmark_domain_id,
            )
            .await?;

        tracing::info!(
            "Postmark domain created successfully for: {}",
            self.custom_domain
        );

        let mut updated_domain_verification_records = domain_verification_records;
        updated_domain_verification_records.frontend_hostname_id = frontend_hostname_id;
        updated_domain_verification_records.backend_hostname_id = backend_hostname_id;
        DeploymentDomainVerificationUpdate::builder()
            .deployment_id(deployment_row.id)
            .domain_verification_records(json_value(&updated_domain_verification_records)?)
            .execute_with_db(tx.as_mut())
            .await?;

        tracing::info!(
            "Successfully created production deployment for domain: {} with hostnames: {}, {}",
            self.custom_domain,
            frontend_hostname,
            backend_hostname
        );

        tx.commit().await?;
        build_production_deployment_model(ProductionDeploymentModelInput {
            id: deployment_row.id,
            created_at: deployment_row.created_at,
            updated_at: deployment_row.updated_at,
            maintenance_mode: deployment_row.maintenance_mode,
            backend_host: deployment_row.backend_host,
            frontend_host: deployment_row.frontend_host,
            publishable_key: deployment_row.publishable_key,
            project_id: deployment_row.project_id,
            mode: deployment_row.mode,
            mail_from_host: deployment_row.mail_from_host,
            domain_verification_records: Some(updated_domain_verification_records),
            email_verification_records: Some(email_verification_records),
            email_provider: deployment_row.email_provider,
            custom_smtp_config: deployment_row.custom_smtp_config,
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
