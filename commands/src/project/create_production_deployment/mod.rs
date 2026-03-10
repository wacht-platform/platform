use super::*;

mod external;
use external::*;

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
        ensure_no_social_methods_requested(&self.auth_methods)?;

        let key_material = generate_deployment_key_material().await?;
        let project = load_project_for_production(self.project_id, tx.as_mut()).await?;

        ensure_billing_status_active(&project.status, "deployment")?;
        ensure_production_deployment_is_unique(self.project_id, &self.custom_domain, tx.as_mut())
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

        let (postmark_domain_id, email_verification_records) = setup_postmark_email_verification(
            deps,
            tx.as_mut(),
            deployment_row.id,
            &hosts.mail_from_host,
        )
        .await?;

        let (frontend_hostname_id, backend_hostname_id) = provision_custom_hostnames(
            deps,
            &frontend_hostname,
            &backend_hostname,
            &self.custom_domain,
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
