use crate::error::AppError;
use models::{DnsRecord, DomainVerificationRecords};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct CloudflareResponse<T> {
    pub success: bool,
    pub errors: Vec<CloudflareError>,
    pub messages: Vec<String>,
    pub result: Option<T>,
}

#[derive(Deserialize)]
pub struct CloudflareError {
    pub code: u32,
    pub message: String,
}

#[derive(Deserialize)]
pub struct CustomHostname {
    pub id: String,
    pub hostname: String,
    pub custom_origin_server: String,
    pub status: String,
    pub verification_errors: Option<Vec<String>>,
}

#[derive(Clone)]
pub struct CloudflareService {
    api_key: String,
    zone_id: String,
    client: reqwest::Client,
}

impl CloudflareService {
    pub fn new(api_key: String, zone_id: String) -> Self {
        let client = reqwest::Client::new();
        Self {
            api_key,
            zone_id,
            client,
        }
    }

    pub async fn create_custom_hostname(
        &self,
        hostname: &str,
        origin_server: &str,
    ) -> Result<CustomHostname, AppError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames",
            self.zone_id
        );

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&json!({
                "hostname": hostname,
                "custom_origin_server": origin_server,
                "ssl": json!({
                    "type": "dv",
                    "method": "http"
                })
            }))
            .send()
            .await
            .map_err(|e| AppError::External(format!("Cloudflare API request failed: {}", e)))?;

        let data = response.text().await.unwrap();

        let cloudflare_response: CloudflareResponse<CustomHostname> = serde_json::from_str(&data)
            .map_err(|e| {
            AppError::External(format!("Failed to parse Cloudflare response: {}", e))
        })?;

        if !cloudflare_response.success {
            let error_messages: Vec<String> = cloudflare_response
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.code, e.message))
                .collect();
            return Err(AppError::External(format!(
                "Cloudflare API errors: {}",
                error_messages.join(", ")
            )));
        }

        cloudflare_response.result.ok_or_else(|| {
            AppError::External("Cloudflare API returned success but no result".to_string())
        })
    }

    pub async fn delete_custom_hostname(&self, hostname_id: &str) -> Result<(), AppError> {
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames/{}",
            self.zone_id, hostname_id
        );

        let response = self
            .client
            .delete(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| AppError::External(format!("Cloudflare API request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AppError::External(format!(
                "Cloudflare API error ({})",
                response.status()
            )));
        }

        Ok(())
    }

    /// Generate DNS records required for custom hostname verification
    pub fn generate_domain_verification_records(
        &self,
        frontend_hostname: &str,
        backend_hostname: &str,
    ) -> DomainVerificationRecords {
        let mut records = DomainVerificationRecords::default();

        // Add CNAME records for custom hostnames
        records.custom_hostname_verification.push(DnsRecord {
            name: frontend_hostname.to_string(),
            record_type: "CNAME".to_string(),
            value: "accounts.wacht.services".to_string(),

            verified: false,
            verification_attempted_at: None,
            last_verified_at: None,
        });

        records.custom_hostname_verification.push(DnsRecord {
            name: backend_hostname.to_string(),
            record_type: "CNAME".to_string(),
            value: "frontend.wacht.services".to_string(),

            verified: false,
            verification_attempted_at: None,
            last_verified_at: None,
        });

        records
    }

    pub async fn check_domain_verification_status(&self, domain: &str) -> Result<bool, AppError> {
        // For domain verification, we check if the specific CNAME record exists and points to the correct value
        let url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/dns_records?name={}&type=CNAME",
            self.zone_id, domain
        );

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| AppError::External(format!("Cloudflare API error: {}", e)))?;

        let cloudflare_response: CloudflareResponse<Vec<serde_json::Value>> =
            response.json().await.map_err(|e| {
                AppError::External(format!("Failed to parse Cloudflare response: {}", e))
            })?;

        if !cloudflare_response.success {
            let error_messages: Vec<String> = cloudflare_response
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.code, e.message))
                .collect();
            return Err(AppError::External(format!(
                "Cloudflare API errors: {}",
                error_messages.join(", ")
            )));
        }

        // Check if we found any CNAME record for this domain
        // If Cloudflare has the record, it means it's properly configured
        if let Some(records) = cloudflare_response.result {
            for record in &records {
                if let Some(name) = record.get("name").and_then(|n| n.as_str()) {
                    if name == domain {
                        // If the record exists in Cloudflare's zone, it's verified
                        if let Some(content) = record.get("content").and_then(|c| c.as_str()) {
                            tracing::info!(
                                "Domain verification found for {}: CNAME -> {}",
                                domain,
                                content
                            );
                        } else {
                            tracing::info!(
                                "Domain verification found for {}: record exists",
                                domain
                            );
                        }
                        return Ok(true);
                    }
                }
            }
        }

        tracing::info!("Domain verification not found for {}", domain);
        Ok(false)
    }

    pub async fn check_custom_hostname_status(&self, hostname: &str) -> Result<bool, AppError> {
        tracing::info!("Checking custom hostname status for: {}", hostname);

        // First, get all custom hostnames to find the ID for this hostname
        let list_url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames?hostname={}",
            self.zone_id, hostname
        );

        tracing::info!("Making Cloudflare API call to: {}", list_url);

        let list_response = self
            .client
            .get(&list_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Cloudflare API call failed: {}", e);
                AppError::External(format!("Cloudflare API error: {}", e))
            })?;

        let list_cloudflare_response: CloudflareResponse<Vec<CustomHostname>> =
            list_response.json().await.map_err(|e| {
                tracing::error!("Failed to parse Cloudflare response: {}", e);
                AppError::External(format!("Failed to parse Cloudflare response: {}", e))
            })?;

        tracing::info!(
            "Cloudflare API response success: {}",
            list_cloudflare_response.success
        );

        if !list_cloudflare_response.success {
            let error_messages: Vec<String> = list_cloudflare_response
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.code, e.message))
                .collect();
            tracing::error!("Cloudflare API errors: {}", error_messages.join(", "));
            return Err(AppError::External(format!(
                "Cloudflare API errors: {}",
                error_messages.join(", ")
            )));
        }

        // Find the custom hostname ID
        let hostname_id = if let Some(hostnames) = list_cloudflare_response.result {
            tracing::info!("Found {} custom hostnames", hostnames.len());
            for (i, h) in hostnames.iter().enumerate() {
                tracing::info!("Custom hostname {}: {} (id: {})", i, h.hostname, h.id);
            }

            hostnames
                .iter()
                .find(|h| h.hostname == hostname)
                .map(|h| h.id.clone())
        } else {
            tracing::warn!("No custom hostnames returned from Cloudflare");
            None
        };

        let hostname_id = match hostname_id {
            Some(id) => {
                tracing::info!("Found custom hostname ID for {}: {}", hostname, id);
                id
            }
            None => {
                tracing::warn!("Custom hostname {} not found in Cloudflare", hostname);
                return Ok(false);
            }
        };

        // Now get the detailed status using the hostname ID
        let detail_url = format!(
            "https://api.cloudflare.com/client/v4/zones/{}/custom_hostnames/{}",
            self.zone_id, hostname_id
        );

        tracing::info!("Getting detailed status from: {}", detail_url);

        let detail_response = self
            .client
            .get(&detail_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| {
                tracing::error!("Cloudflare detail API call failed: {}", e);
                AppError::External(format!("Cloudflare API error: {}", e))
            })?;

        let detail_cloudflare_response: CloudflareResponse<serde_json::Value> =
            detail_response.json().await.map_err(|e| {
                tracing::error!("Failed to parse Cloudflare detail response: {}", e);
                AppError::External(format!("Failed to parse Cloudflare response: {}", e))
            })?;

        tracing::info!(
            "Cloudflare detail API response success: {}",
            detail_cloudflare_response.success
        );

        if !detail_cloudflare_response.success {
            let error_messages: Vec<String> = detail_cloudflare_response
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.code, e.message))
                .collect();
            tracing::error!(
                "Cloudflare detail API errors: {}",
                error_messages.join(", ")
            );
            return Err(AppError::External(format!(
                "Cloudflare API errors: {}",
                error_messages.join(", ")
            )));
        }

        // Check the status field
        if let Some(result) = detail_cloudflare_response.result {
            tracing::info!(
                "Custom hostname detail result: {}",
                serde_json::to_string_pretty(&result).unwrap_or_default()
            );

            if let Some(status) = result.get("status").and_then(|s| s.as_str()) {
                tracing::info!("Custom hostname {} status: {}", hostname, status);
                let is_active = status == "active";
                tracing::info!("Is hostname {} active? {}", hostname, is_active);
                return Ok(is_active);
            } else {
                tracing::warn!("No status field found in custom hostname response");
            }
        } else {
            tracing::warn!("No result field in custom hostname response");
        }

        tracing::warn!("Custom hostname check failed for {}", hostname);
        Ok(false)
    }
}
