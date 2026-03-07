use chrono::Utc;
use common::DnsVerificationService;
use common::{HasDbRouter, HasDnsVerificationService, HasIdGenerator, error::AppError};
use models::DnsRecord;
use models::organization_domain::OrganizationDomain;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Serialize, Deserialize, Validate)]
pub struct CreateOrganizationDomainRequest {
    #[serde(default)]
    pub organization_id: i64,
    #[validate(length(min = 1, message = "Domain is required"))]
    pub fqdn: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateOrganizationDomainResponse {
    pub domain: OrganizationDomain,
    pub verification_token: String,
}

pub struct CreateOrganizationDomainCommand {
    domain_id: Option<i64>,
    deployment_id: i64,
    request: CreateOrganizationDomainRequest,
}

#[derive(Default)]
pub struct CreateOrganizationDomainCommandBuilder {
    domain_id: Option<i64>,
    deployment_id: Option<i64>,
    request: Option<CreateOrganizationDomainRequest>,
}

impl CreateOrganizationDomainCommand {
    pub fn builder() -> CreateOrganizationDomainCommandBuilder {
        CreateOrganizationDomainCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: CreateOrganizationDomainRequest) -> Self {
        Self {
            domain_id: None,
            deployment_id,
            request,
        }
    }

    pub fn with_domain_id(mut self, domain_id: i64) -> Self {
        self.domain_id = Some(domain_id);
        self
    }
}

impl CreateOrganizationDomainCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<CreateOrganizationDomainResponse, AppError>
    where
        D: HasDbRouter + HasIdGenerator,
    {
        let domain_id = self
            .domain_id
            .unwrap_or(deps.id_generator().next_id()? as i64);
        self.run_with_domain_id(deps.db_router().writer(), domain_id)
            .await
    }

    async fn run_with_domain_id(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
        domain_id: i64,
    ) -> Result<CreateOrganizationDomainResponse, AppError> {
        let mut conn = acquirer.acquire().await?;
        self.request
            .validate()
            .map_err(|e| AppError::Validation(e.to_string()))?;

        let mut bytes = [0u8; 32];
        use rand::RngCore;
        rand::rng().fill_bytes(&mut bytes);
        let token_base = hex::encode(bytes);
        let verification_token = format!("wacht-domain-verification={}", token_base);

        let domain = sqlx::query_as!(
            OrganizationDomain,
            r#"
            INSERT INTO organization_domains (
                id,
                organization_id,
                deployment_id,
                fqdn,
                verified,
                verification_dns_record_type,
                verification_dns_record_name,
                verification_dns_record_data,
                verification_attempts,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, false, 'TXT', '_wacht-verification', $5, 0, $6, $6)
            RETURNING *
            "#,
            domain_id,
            self.request.organization_id,
            self.deployment_id,
            self.request.fqdn,
            verification_token,
            Utc::now()
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db_err) if db_err.code().as_deref() == Some("23505") => {
                AppError::Conflict("Domain already exists for this organization".to_string())
            }
            _ => AppError::Database(e),
        })?;

        Ok(CreateOrganizationDomainResponse {
            domain,
            verification_token,
        })
    }

    pub async fn execute_with_db(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
    ) -> Result<CreateOrganizationDomainResponse, AppError> {
        let domain_id = self
            .domain_id
            .ok_or_else(|| AppError::Validation("domain_id is required".to_string()))?;
        self.run_with_domain_id(acquirer, domain_id).await
    }
}

impl CreateOrganizationDomainCommandBuilder {
    pub fn domain_id(mut self, domain_id: i64) -> Self {
        self.domain_id = Some(domain_id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: CreateOrganizationDomainRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<CreateOrganizationDomainCommand, AppError> {
        Ok(CreateOrganizationDomainCommand {
            domain_id: self.domain_id,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteOrganizationDomainRequest {
    pub organization_id: i64,
    pub domain_id: i64,
}

pub struct DeleteOrganizationDomainCommand {
    deployment_id: i64,
    request: DeleteOrganizationDomainRequest,
}

#[derive(Default)]
pub struct DeleteOrganizationDomainCommandBuilder {
    deployment_id: Option<i64>,
    request: Option<DeleteOrganizationDomainRequest>,
}

impl DeleteOrganizationDomainCommand {
    pub fn builder() -> DeleteOrganizationDomainCommandBuilder {
        DeleteOrganizationDomainCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: DeleteOrganizationDomainRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl DeleteOrganizationDomainCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<(), AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM organization_domains
            WHERE id = $1 AND organization_id = $2 AND deployment_id = $3
            "#,
            self.request.domain_id,
            self.request.organization_id,
            self.deployment_id
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Domain not found".to_string()));
        }

        Ok(())
    }
}

impl DeleteOrganizationDomainCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: DeleteOrganizationDomainRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<DeleteOrganizationDomainCommand, AppError> {
        Ok(DeleteOrganizationDomainCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyOrganizationDomainRequest {
    pub organization_id: i64,
    pub domain_id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VerifyOrganizationDomainResponse {
    pub verified: bool,
    pub message: Option<String>,
}

pub struct VerifyOrganizationDomainCommand {
    deployment_id: i64,
    request: VerifyOrganizationDomainRequest,
}

#[derive(Default)]
pub struct VerifyOrganizationDomainCommandBuilder {
    deployment_id: Option<i64>,
    request: Option<VerifyOrganizationDomainRequest>,
}

impl VerifyOrganizationDomainCommand {
    pub fn builder() -> VerifyOrganizationDomainCommandBuilder {
        VerifyOrganizationDomainCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, request: VerifyOrganizationDomainRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl VerifyOrganizationDomainCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<VerifyOrganizationDomainResponse, AppError>
    where
        D: HasDbRouter + HasDnsVerificationService,
    {
        self.run_with_deps(deps.db_router().writer(), deps.dns_verification_service())
            .await
    }

    async fn run_with_deps(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
        dns_verification_service: &DnsVerificationService,
    ) -> Result<VerifyOrganizationDomainResponse, AppError> {
        let mut conn = acquirer.acquire().await?;
        // Fetch the domain
        let domain = sqlx::query_as!(
            OrganizationDomain,
            r#"
            SELECT * FROM organization_domains
            WHERE id = $1 AND organization_id = $2 AND deployment_id = $3
            "#,
            self.request.domain_id,
            self.request.organization_id,
            self.deployment_id
        )
        .fetch_one(&mut *conn)
        .await?;

        if domain.verified {
            return Ok(VerifyOrganizationDomainResponse {
                verified: true,
                message: Some("Domain is already verified".to_string()),
            });
        }

        // Increment verification attempts
        sqlx::query!(
            r#"
            UPDATE organization_domains
            SET verification_attempts = verification_attempts + 1
            WHERE id = $1
            "#,
            domain.id
        )
        .execute(&mut *conn)
        .await?;

        let txt_record_name = format!(
            "{}.{}",
            domain
                .verification_dns_record_name
                .as_deref()
                .unwrap_or("_wacht-verification"),
            domain.fqdn
        );

        let expected_token = domain
            .verification_dns_record_data
            .as_ref()
            .ok_or_else(|| AppError::Internal("No verification token found".to_string()))?;

        // Create a DnsRecord for verification
        let dns_record = DnsRecord {
            name: txt_record_name.clone(),
            record_type: "TXT".to_string(),
            value: expected_token.clone(),
            verified: false,
            verification_attempted_at: None,
            last_verified_at: None,
        };

        // Verify DNS TXT record using the service
        match dns_verification_service
            .verify_dns_record(&dns_record)
            .await
        {
            Ok(true) => {
                // Mark as verified
                sqlx::query!(
                    r#"
                    UPDATE organization_domains
                    SET verified = true, updated_at = $1
                    WHERE id = $2
                    "#,
                    Utc::now(),
                    domain.id
                )
                .execute(&mut *conn)
                .await?;

                Ok(VerifyOrganizationDomainResponse {
                    verified: true,
                    message: Some("Domain verified successfully".to_string()),
                })
            }
            Ok(false) | Err(_) => Ok(VerifyOrganizationDomainResponse {
                verified: false,
                message: Some(format!(
                    "Verification failed. Please ensure TXT record '{}' is set to '{}'",
                    txt_record_name, expected_token
                )),
            }),
        }
    }
}

impl VerifyOrganizationDomainCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn request(mut self, request: VerifyOrganizationDomainRequest) -> Self {
        self.request = Some(request);
        self
    }

    pub fn build(self) -> Result<VerifyOrganizationDomainCommand, AppError> {
        Ok(VerifyOrganizationDomainCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            request: self
                .request
                .ok_or_else(|| AppError::Validation("request is required".to_string()))?,
        })
    }
}
