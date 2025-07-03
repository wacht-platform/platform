use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct DeploymentRestrictions {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
    pub deployment_id: i64,
    pub allowlist_enabled: bool,
    pub blocklist_enabled: bool,
    pub block_subaddresses: bool,
    pub block_disposable_emails: bool,
    pub block_voip_numbers: bool,
    pub country_restrictions: CountryRestrictions,
    pub banned_keywords: Vec<String>,
    pub allowlisted_resources: Vec<String>,
    pub blocklisted_resources: Vec<String>,
    pub sign_up_mode: DeploymentRestrictionsSignUpMode,
    pub waitlist_collect_names: bool,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct CountryRestrictions {
    pub enabled: bool,
    pub country_codes: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub enum DeploymentRestrictionsSignUpMode {
    #[serde(rename = "public")]
    #[default]
    Public,
    #[serde(rename = "restricted")]
    Restricted,
    #[serde(rename = "waitlist")]
    Waitlist,
}

impl FromStr for DeploymentRestrictionsSignUpMode {
    type Err = AppError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "public" => Ok(DeploymentRestrictionsSignUpMode::Public),
            "restricted" => Ok(DeploymentRestrictionsSignUpMode::Restricted),
            "waitlist" => Ok(DeploymentRestrictionsSignUpMode::Waitlist),
            _ => Err(AppError::Serialization(format!(
                "Invalid sign up mode: {}",
                s
            ))),
        }
    }
}

impl ToString for DeploymentRestrictionsSignUpMode {
    fn to_string(&self) -> String {
        match self {
            DeploymentRestrictionsSignUpMode::Public => "public".to_string(),
            DeploymentRestrictionsSignUpMode::Restricted => "restricted".to_string(),
            DeploymentRestrictionsSignUpMode::Waitlist => "waitlist".to_string(),
        }
    }
}
