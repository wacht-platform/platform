use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct CustomSigningKey {
    pub enabled: bool,
    pub key: String,
    pub algorithm: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentJwtTemplate {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub name: String,
    pub token_lifetime: i64,
    pub allowed_clock_skew: i64,
    pub custom_signing_key: Option<CustomSigningKey>,
    pub template: Value,
    pub deployment_id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
