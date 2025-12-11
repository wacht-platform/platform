use crate::Command;
use chrono::Utc;
use common::error::AppError;
use common::state::AppState;
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
    pub deployment_id: i64,
    pub request: CreateOrganizationDomainRequest,
}

impl CreateOrganizationDomainCommand {
    pub fn new(deployment_id: i64, request: CreateOrganizationDomainRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for CreateOrganizationDomainCommand {
    type Output = CreateOrganizationDomainResponse;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
            app_state.sf.next_id()? as i64,
            self.request.organization_id,
            self.deployment_id,
            self.request.fqdn,
            verification_token,
            Utc::now()
        )
        .fetch_one(&app_state.db_pool)
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteOrganizationDomainRequest {
    pub organization_id: i64,
    pub domain_id: i64,
}

pub struct DeleteOrganizationDomainCommand {
    pub deployment_id: i64,
    pub request: DeleteOrganizationDomainRequest,
}

impl DeleteOrganizationDomainCommand {
    pub fn new(deployment_id: i64, request: DeleteOrganizationDomainRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for DeleteOrganizationDomainCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query!(
            r#"
            DELETE FROM organization_domains
            WHERE id = $1 AND organization_id = $2 AND deployment_id = $3
            "#,
            self.request.domain_id,
            self.request.organization_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Domain not found".to_string()));
        }

        Ok(())
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
    pub deployment_id: i64,
    pub request: VerifyOrganizationDomainRequest,
}

impl VerifyOrganizationDomainCommand {
    pub fn new(deployment_id: i64, request: VerifyOrganizationDomainRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for VerifyOrganizationDomainCommand {
    type Output = VerifyOrganizationDomainResponse;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_one(&app_state.db_pool)
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
        .execute(&app_state.db_pool)
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
        match app_state
            .dns_verification_service
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
                .execute(&app_state.db_pool)
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
