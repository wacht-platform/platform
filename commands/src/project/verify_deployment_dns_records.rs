use super::*;
pub struct VerifyDeploymentDnsRecordsCommand {
    deployment_id: i64,
}

#[derive(Default)]
pub struct VerifyDeploymentDnsRecordsCommandBuilder {
    deployment_id: Option<i64>,
}

impl VerifyDeploymentDnsRecordsCommand {
    pub fn builder() -> VerifyDeploymentDnsRecordsCommandBuilder {
        VerifyDeploymentDnsRecordsCommandBuilder::default()
    }

    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_deps(
        self,
        deps: &VerifyDeploymentDnsDeps<'_>,
    ) -> Result<Deployment, AppError> {
        // Get current deployment with DNS records
        let deployment_row = DeploymentByIdQuery::builder()
            .deployment_id(self.deployment_id)
            .execute_with_db(deps.db_router.writer())
            .await?;

        // Extract domain from backend host for email verification
        let domain = if deployment_row.backend_host.starts_with("frontend.") {
            deployment_row
                .backend_host
                .strip_prefix("frontend.")
                .unwrap_or(&deployment_row.backend_host)
        } else {
            &deployment_row.backend_host
        };

        // Get existing records from database or create new ones
        let mut domain_verification_records = deployment_row
            .domain_verification_records
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_else(|| {
                deps.cloudflare_service
                    .generate_domain_verification_records(
                        &deployment_row.frontend_host,
                        &deployment_row.backend_host,
                    )
            });

        let mut email_verification_records = deployment_row
            .email_verification_records
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        deps.dns_verification_service
            .verify_domain_records(&mut domain_verification_records, deps.cloudflare_service)
            .await
            .map_err(|e| {
                tracing::warn!("Failed to verify domain records: {}", e);
                e
            })
            .unwrap_or(());

        deps.dns_verification_service
            .verify_email_records(&mut email_verification_records)
            .await
            .map_err(|e| {
                tracing::warn!("Failed to verify email records: {}", e);
                e
            })
            .unwrap_or(());

        tracing::info!("DNS verification completed for domain: {}", domain);

        let domain_verified = deps
            .dns_verification_service
            .are_domain_records_verified(&domain_verification_records);
        let email_verified = deps
            .dns_verification_service
            .are_email_records_verified(&email_verification_records);

        let verification_status = if domain_verified && email_verified {
            "verified"
        } else {
            "in_progress"
        };

        DeploymentDnsRecordsUpdate::builder()
            .deployment_id(self.deployment_id)
            .domain_verification_records(
                serde_json::to_value(&domain_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .email_verification_records(
                serde_json::to_value(&email_verification_records)
                    .map_err(|e| AppError::Serialization(e.to_string()))?,
            )
            .execute_with_db(deps.db_router.writer())
            .await?;

        let _final_verification_status = match verification_status {
            "verified" => VerificationStatus::Verified,
            "in_progress" => VerificationStatus::InProgress,
            _ => VerificationStatus::Pending,
        };

        tracing::info!(
            "DNS verification completed for deployment {}: domain_verified={}, email_verified={}, status={}",
            self.deployment_id,
            domain_verified,
            email_verified,
            verification_status
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
            domain_verification_records: Some(domain_verification_records),
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

impl VerifyDeploymentDnsRecordsCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn build(self) -> Result<VerifyDeploymentDnsRecordsCommand, AppError> {
        Ok(VerifyDeploymentDnsRecordsCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
        })
    }
}
