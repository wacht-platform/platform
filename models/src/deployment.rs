use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::{
    DeploymentAuthSettings, DeploymentB2bSettingsWithRoles, DeploymentRestrictions,
    DeploymentUISettings,
};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Pending,
    InProgress,
    Verified,
    Failed,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub record_type: String,
    pub value: String,
    pub verified: bool,
    pub verification_attempted_at: Option<DateTime<Utc>>,
    pub last_verified_at: Option<DateTime<Utc>>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct DomainVerificationRecords {
    pub cloudflare_verification: Vec<DnsRecord>,
    pub custom_hostname_verification: Vec<DnsRecord>,
    pub frontend_hostname_id: Option<String>,
    pub backend_hostname_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct EmailVerificationRecords {
    pub dkim_records: Vec<DnsRecord>,
    pub return_path_records: Vec<DnsRecord>,
    pub postmark_domain_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum EmailProvider {
    #[default]
    Postmark,
    CustomSmtp,
}

impl fmt::Display for EmailProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmailProvider::Postmark => write!(f, "postmark"),
            EmailProvider::CustomSmtp => write!(f, "custom_smtp"),
        }
    }
}

impl From<String> for EmailProvider {
    fn from(value: String) -> Self {
        match value.to_lowercase().as_str() {
            "custom_smtp" => EmailProvider::CustomSmtp,
            _ => EmailProvider::Postmark,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct CustomSmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    #[serde(skip_serializing)]
    pub password: String,
    pub from_email: String,
    pub use_tls: bool,
    pub verified: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentMode {
    Production,
    Staging,
}

impl From<String> for DeploymentMode {
    fn from(value: String) -> Self {
        match value.to_lowercase().as_str() {
            "production" => DeploymentMode::Production,
            "staging" => DeploymentMode::Staging,
            _ => panic!("Invalid deployment mode: {}", value),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Deployment {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub maintenance_mode: bool,
    pub backend_host: String,
    pub frontend_host: String,
    pub mail_from_host: String,
    pub publishable_key: String,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub project_id: i64,
    pub mode: DeploymentMode,
    pub domain_verification_records: Option<DomainVerificationRecords>,
    pub email_verification_records: Option<EmailVerificationRecords>,
    #[serde(default)]
    pub email_provider: EmailProvider,
    pub custom_smtp_config: Option<CustomSmtpConfig>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentWithSettings {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub maintenance_mode: bool,
    pub backend_host: String,
    pub frontend_host: String,
    pub mail_from_host: String,
    pub publishable_key: String,
    pub mode: DeploymentMode,
    pub auth_settings: Option<DeploymentAuthSettings>,
    pub ui_settings: Option<DeploymentUISettings>,
    pub b2b_settings: Option<DeploymentB2bSettingsWithRoles>,
    pub restrictions: Option<DeploymentRestrictions>,
    pub domain_verification_records: Option<DomainVerificationRecords>,
    pub email_verification_records: Option<EmailVerificationRecords>,
    #[serde(default)]
    pub email_provider: EmailProvider,
    pub custom_smtp_config: Option<CustomSmtpConfig>,
}
