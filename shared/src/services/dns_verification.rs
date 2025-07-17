use crate::error::AppError;
use crate::models::{DnsRecord, DomainVerificationRecords, EmailVerificationRecords};
use chrono::Utc;
use serde::Deserialize;

#[derive(Clone)]
pub struct DnsVerificationService {
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct DnsResponse {
    #[serde(rename = "Answer")]
    answer: Option<Vec<DnsAnswer>>,
}

#[derive(Deserialize)]
struct DnsAnswer {
    data: String,
}

impl DnsVerificationService {
    pub fn new() -> Self {
        let client = reqwest::Client::new();
        Self { client }
    }

    pub async fn verify_dns_record(&self, record: &DnsRecord) -> Result<bool, AppError> {
        let record_type_num = match record.record_type.as_str() {
            "A" => 1,
            "CNAME" => 5,
            "TXT" => 16,
            "MX" => 15,
            _ => {
                return Err(AppError::BadRequest(format!(
                    "Unsupported DNS record type: {}",
                    record.record_type
                )));
            }
        };

        let url = format!(
            "https://dns.google/resolve?name={}&type={}",
            record.name, record_type_num
        );

        let response = self.client
            .get(&url)
            .header("Accept", "application/dns-json")
            .send()
            .await
            .map_err(|e| AppError::External(format!("DNS query failed: {}", e)))?;

        let dns_response: DnsResponse = response
            .json()
            .await
            .map_err(|e| AppError::External(format!("Failed to parse DNS response: {}", e)))?;

        if let Some(answers) = dns_response.answer {
            for answer in answers {
                if self.matches_expected_value(&answer.data, &record.value, &record.record_type) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn matches_expected_value(&self, actual: &str, expected: &str, record_type: &str) -> bool {
        match record_type {
            "CNAME" => {
                let actual_normalized = actual.trim_end_matches('.');
                let expected_normalized = expected.trim_end_matches('.');
                actual_normalized.eq_ignore_ascii_case(expected_normalized)
            }
            "TXT" => {
                let actual_unquoted = actual.trim_matches('"');
                actual_unquoted == expected
            }
            "A" => actual == expected,
            "MX" => {
                if let Some(hostname) = actual.split_whitespace().nth(1) {
                    let hostname_normalized = hostname.trim_end_matches('.');
                    let expected_normalized = expected.trim_end_matches('.');
                    hostname_normalized.eq_ignore_ascii_case(expected_normalized)
                } else {
                    false
                }
            }
            _ => actual == expected,
        }
    }

    pub async fn verify_domain_records(
        &self,
        records: &mut DomainVerificationRecords,
        cloudflare_service: &crate::services::cloudflare::CloudflareService,
    ) -> Result<(), AppError> {
        // For Cloudflare verification records, first try custom hostname API, then DNS records
        for record in &mut records.cloudflare_verification {
            tracing::info!(
                "Starting verification for Cloudflare record: {}",
                record.name
            );
            record.verification_attempted_at = Some(Utc::now());

            // First try checking as a custom hostname (for domains like accounts.wacht.dev)
            tracing::info!("Attempting custom hostname check for: {}", record.name);
            match cloudflare_service.check_custom_hostname_status(&record.name).await {
                Ok(verified) => {
                    record.verified = verified;
                    if verified {
                        record.last_verified_at = Some(Utc::now());
                    }
                    tracing::info!(
                        "Cloudflare custom hostname verification for {}: {} ✅",
                        record.name,
                        verified
                    );
                }
                Err(e) => {
                    tracing::info!(
                        "Custom hostname check failed for {}, trying DNS records: {}",
                        record.name,
                        e
                    );

                    // Fallback to checking DNS records
                    match cloudflare_service.check_domain_verification_status(&record.name).await {
                        Ok(verified) => {
                            record.verified = verified;
                            if verified {
                                record.last_verified_at = Some(Utc::now());
                            }
                            tracing::info!(
                                "Cloudflare DNS verification for {}: {} ✅",
                                record.name,
                                verified
                            );
                        }
                        Err(dns_e) => {
                            tracing::warn!(
                                "Cloudflare DNS check failed for {}, trying manual DNS: {}",
                                record.name,
                                dns_e
                            );
                            // Final fallback to manual DNS lookup
                            match self.verify_dns_record(record).await {
                                Ok(verified) => {
                                    record.verified = verified;
                                    if verified {
                                        record.last_verified_at = Some(Utc::now());
                                    }
                                    tracing::info!(
                                        "Manual DNS verification for {}: {} ✅",
                                        record.name,
                                        verified
                                    );
                                }
                                Err(manual_e) => {
                                    tracing::warn!(
                                        "Manual DNS fallback also failed for {}: {} ❌",
                                        record.name,
                                        manual_e
                                    );
                                    record.verified = false;
                                }
                            }
                        }
                    }
                }
            }

            tracing::info!(
                "Final verification result for {}: {}",
                record.name,
                record.verified
            );
        }

        // For custom hostname verification, use Cloudflare API to check hostname status
        for record in &mut records.custom_hostname_verification {
            record.verification_attempted_at = Some(Utc::now());

            // Use Cloudflare API to check custom hostname status
            match cloudflare_service.check_custom_hostname_status(&record.name).await {
                Ok(verified) => {
                    record.verified = verified;
                    if verified {
                        record.last_verified_at = Some(Utc::now());
                    }
                    tracing::info!(
                        "Cloudflare custom hostname verification for {}: {}",
                        record.name,
                        verified
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to check Cloudflare custom hostname for {}: {}",
                        record.name,
                        e
                    );
                    // Fallback to DNS lookup if Cloudflare API fails
                    match self.verify_dns_record(record).await {
                        Ok(verified) => {
                            record.verified = verified;
                            if verified {
                                record.last_verified_at = Some(Utc::now());
                            }
                        }
                        Err(dns_e) => {
                            tracing::warn!(
                                "DNS fallback also failed for {}: {}",
                                record.name,
                                dns_e
                            );
                            record.verified = false;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn verify_email_records(
        &self,
        records: &mut EmailVerificationRecords,
    ) -> Result<(), AppError> {
        for record in &mut records.dkim_records {
            if !record.verified {
                record.verification_attempted_at = Some(chrono::Utc::now());
                if let Ok(verified) = self.verify_dns_record(record).await {
                    record.verified = verified;
                    if verified {
                        record.last_verified_at = Some(chrono::Utc::now());
                    }
                }
            }
        }

        for record in &mut records.return_path_records {
            if !record.verified {
                record.verification_attempted_at = Some(chrono::Utc::now());
                if let Ok(verified) = self.verify_dns_record(record).await {
                    record.verified = verified;
                    if verified {
                        record.last_verified_at = Some(chrono::Utc::now());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn are_domain_records_verified(&self, records: &DomainVerificationRecords) -> bool {
        let cloudflare_verified = records.cloudflare_verification.iter().all(|r| r.verified);
        let hostname_verified = records
            .custom_hostname_verification
            .iter()
            .all(|r| r.verified);

        cloudflare_verified && hostname_verified
    }

    pub fn are_email_records_verified(&self, records: &EmailVerificationRecords) -> bool {
        let dkim_verified = records.dkim_records.iter().all(|r| r.verified);
        let return_path_verified = records.return_path_records.iter().all(|r| r.verified);

        dkim_verified && return_path_verified
    }
}
