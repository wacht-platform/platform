use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{OrganizationRole, Workspace};

#[derive(Serialize, Deserialize, Clone)]
pub struct OrganizationDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub image_url: String,
    pub description: String,
    pub member_count: i64,
    pub public_metadata: Value,
    pub private_metadata: Value,
    pub roles: Vec<OrganizationRole>,
    pub workspaces: Vec<Workspace>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct OrganizationMemberDetails {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub organization_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub user_id: i64,
    pub roles: Vec<OrganizationRole>,
    pub public_metadata: Value,

    // User details
    pub first_name: String,
    pub last_name: String,
    pub username: Option<String>,
    pub primary_email_address: Option<String>,
    pub primary_phone_number: Option<String>,
    pub user_created_at: DateTime<Utc>,
}
