use crate::error::AppError;
use models::{DnsRecord, EmailVerificationRecords};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct PostmarkService {
    account_token: String,
    server_token: String,
    base_url: String,
    client: reqwest::Client,
}

#[derive(Serialize, Deserialize)]
pub struct PostmarkDomain {
    #[serde(rename = "ID")]
    pub id: i64,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "SPFVerified")]
    pub spf_verified: bool,
    #[serde(rename = "SPFHost")]
    pub spf_host: String,
    #[serde(rename = "SPFTextValue")]
    pub spf_text_value: String,
    #[serde(rename = "DKIMVerified")]
    pub dkim_verified: bool,
    #[serde(rename = "WeakDKIM")]
    pub weak_dkim: bool,
    #[serde(rename = "DKIMHost")]
    pub dkim_host: String,
    #[serde(rename = "DKIMTextValue")]
    pub dkim_text_value: String,
    #[serde(rename = "DKIMPendingHost")]
    pub dkim_pending_host: String,
    #[serde(rename = "DKIMPendingTextValue")]
    pub dkim_pending_text_value: String,
    #[serde(rename = "DKIMRevokedHost")]
    pub dkim_revoked_host: String,
    #[serde(rename = "DKIMRevokedTextValue")]
    pub dkim_revoked_text_value: String,
    #[serde(rename = "SafeToRemoveRevokedKeyFromDNS")]
    pub safe_to_remove_revoked_key: bool,
    #[serde(rename = "DKIMUpdateStatus")]
    pub dkim_update_status: String,
    #[serde(rename = "ReturnPathDomain")]
    pub return_path_domain: String,
    #[serde(rename = "ReturnPathDomainVerified")]
    pub return_path_domain_verified: bool,
    #[serde(rename = "ReturnPathDomainCNAMEValue")]
    pub return_path_domain_cname_value: String,
}

#[derive(Serialize)]
pub struct CreateDomainRequest {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "ReturnPathDomain")]
    pub return_path_domain: String,
}

#[derive(Serialize)]
pub struct SendEmailRequest {
    #[serde(rename = "From")]
    pub from: String,
    #[serde(rename = "To")]
    pub to: String,
    #[serde(rename = "Subject")]
    pub subject: String,
    #[serde(rename = "HtmlBody")]
    pub html_body: String,
    #[serde(rename = "TextBody")]
    pub text_body: Option<String>,
    #[serde(rename = "MessageStream")]
    pub message_stream: Option<String>,
}

#[derive(Deserialize)]
pub struct SendEmailResponse {
    #[serde(rename = "To")]
    pub to: String,
    #[serde(rename = "SubmittedAt")]
    pub submitted_at: String,
    #[serde(rename = "MessageID")]
    pub message_id: String,
    #[serde(rename = "ErrorCode")]
    pub error_code: i32,
    #[serde(rename = "Message")]
    pub message: String,
}

impl PostmarkService {
    pub fn new(account_token: String, server_token: String) -> Self {
        let client = reqwest::Client::new();
        Self {
            account_token,
            server_token,
            base_url: "https://api.postmarkapp.com".to_string(),
            client,
        }
    }

    pub async fn create_domain(&self, domain_name: &str) -> Result<PostmarkDomain, AppError> {
        let return_path_domain = format!("rp.{}", domain_name);

        let request = CreateDomainRequest {
            name: domain_name.to_string(),
            return_path_domain,
        };

        let response = self
            .client
            .post(format!("{}/domains", self.base_url))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-Postmark-Account-Token", &self.account_token)
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::External(format!("Failed to create Postmark domain: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AppError::External(format!(
                "Postmark API error: {}",
                error_text
            )));
        }

        let domain: PostmarkDomain = response
            .json()
            .await
            .map_err(|e| AppError::External(format!("Failed to parse Postmark response: {}", e)))?;

        tracing::info!(
            "Successfully created Postmark domain: {} (ID: {})",
            domain.name,
            domain.id
        );
        Ok(domain)
    }

    pub async fn get_domain(&self, domain_id: i64) -> Result<PostmarkDomain, AppError> {
        let response = self
            .client
            .get(format!("{}/domains/{}", self.base_url, domain_id))
            .header("Accept", "application/json")
            .header("X-Postmark-Account-Token", &self.account_token)
            .send()
            .await
            .map_err(|e| AppError::External(format!("Failed to get Postmark domain: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AppError::External(format!(
                "Postmark API error: {}",
                error_text
            )));
        }

        let domain: PostmarkDomain = response
            .json()
            .await
            .map_err(|e| AppError::External(format!("Failed to parse Postmark response: {}", e)))?;

        Ok(domain)
    }

    pub async fn verify_dkim(&self, domain_id: i64) -> Result<PostmarkDomain, AppError> {
        let response = self
            .client
            .put(format!(
                "{}/domains/{}/verifyDkim",
                self.base_url, domain_id
            ))
            .header("Accept", "application/json")
            .header("X-Postmark-Account-Token", &self.account_token)
            .send()
            .await
            .map_err(|e| AppError::External(format!("Failed to verify DKIM: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AppError::External(format!(
                "Postmark DKIM verification error: {}",
                error_text
            )));
        }

        let domain: PostmarkDomain = response
            .json()
            .await
            .map_err(|e| AppError::External(format!("Failed to parse Postmark response: {}", e)))?;

        Ok(domain)
    }

    pub async fn verify_return_path(&self, domain_id: i64) -> Result<PostmarkDomain, AppError> {
        let response = self
            .client
            .put(format!(
                "{}/domains/{}/verifyReturnPath",
                self.base_url, domain_id
            ))
            .header("Accept", "application/json")
            .header("X-Postmark-Account-Token", &self.account_token)
            .send()
            .await
            .map_err(|e| AppError::External(format!("Failed to verify Return-Path: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AppError::External(format!(
                "Postmark Return-Path verification error: {}",
                error_text
            )));
        }

        let domain: PostmarkDomain = response
            .json()
            .await
            .map_err(|e| AppError::External(format!("Failed to parse Postmark response: {}", e)))?;

        Ok(domain)
    }

    pub async fn send_email(
        &self,
        from: &str,
        to: &str,
        subject: &str,
        html_body: &str,
        text_body: Option<&str>,
    ) -> Result<SendEmailResponse, AppError> {
        let request = SendEmailRequest {
            from: from.to_string(),
            to: to.to_string(),
            subject: subject.to_string(),
            html_body: html_body.to_string(),
            text_body: text_body.map(|s| s.to_string()),
            message_stream: Some("outbound".to_string()),
        };

        let response = self
            .client
            .post(format!("{}/email", self.base_url))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("X-Postmark-Server-Token", &self.server_token)
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::External(format!("Failed to send email via Postmark: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AppError::External(format!(
                "Postmark email sending error: {}",
                error_text
            )));
        }

        let email_response: SendEmailResponse = response
            .json()
            .await
            .map_err(|e| AppError::External(format!("Failed to parse Postmark response: {}", e)))?;

        if email_response.error_code != 0 {
            return Err(AppError::External(format!(
                "Postmark email error: {}",
                email_response.message
            )));
        }

        tracing::info!("Successfully sent email via Postmark: {} -> {}", from, to);
        Ok(email_response)
    }

    pub async fn delete_domain(&self, domain_id: i64) -> Result<(), AppError> {
        let response = self
            .client
            .delete(format!("{}/domains/{}", self.base_url, domain_id))
            .header("Accept", "application/json")
            .header("X-Postmark-Account-Token", &self.account_token)
            .send()
            .await
            .map_err(|e| AppError::External(format!("Failed to delete Postmark domain: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(AppError::External(format!(
                "Postmark domain deletion error: {}",
                error_text
            )));
        }

        tracing::info!("Successfully deleted Postmark domain: {}", domain_id);
        Ok(())
    }

    pub fn generate_email_verification_records(
        &self,
        domain: &PostmarkDomain,
    ) -> EmailVerificationRecords {
        let mut records = EmailVerificationRecords::default();

        records.postmark_domain_id = Some(domain.id);

        if !domain.dkim_pending_host.is_empty() && !domain.dkim_pending_text_value.is_empty() {
            records.dkim_records.push(DnsRecord {
                name: domain.dkim_pending_host.clone(),
                record_type: "TXT".to_string(),
                value: domain.dkim_pending_text_value.clone(),
                verified: false,
                verification_attempted_at: None,
                last_verified_at: None,
            });
        }

        if !domain.dkim_host.is_empty() && !domain.dkim_text_value.is_empty() {
            records.dkim_records.push(DnsRecord {
                name: domain.dkim_host.clone(),
                record_type: "TXT".to_string(),
                value: domain.dkim_text_value.clone(),
                verified: domain.dkim_verified,
                verification_attempted_at: None,
                last_verified_at: None,
            });
        }

        if !domain.return_path_domain.is_empty()
            && !domain.return_path_domain_cname_value.is_empty()
        {
            records.return_path_records.push(DnsRecord {
                name: domain.return_path_domain.clone(),
                record_type: "CNAME".to_string(),
                value: domain.return_path_domain_cname_value.clone(),
                verified: domain.return_path_domain_verified,
                verification_attempted_at: None,
                last_verified_at: None,
            });
        }

        records
    }

    pub fn are_records_verified(&self, records: &EmailVerificationRecords) -> bool {
        let dkim_verified = records.dkim_records.iter().all(|r| r.verified);
        let return_path_verified = records.return_path_records.iter().all(|r| r.verified);

        dkim_verified && return_path_verified
    }
}
