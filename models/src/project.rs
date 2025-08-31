use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::Deployment;

#[derive(Clone, Serialize, Deserialize)]
pub struct ProjectWithDeployments {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub image_url: String,
    pub owner_id: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub billing_account_id: i64,
    pub deployments: Vec<Deployment>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub image_url: String,
    pub owner_id: Option<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub billing_account_id: i64,
}
