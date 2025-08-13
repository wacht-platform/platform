use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone)]
pub struct Workspace {
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
}

#[derive(Serialize, Deserialize, Clone)]
pub struct WorkspaceWithOrganizationName {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub image_url: String,
    pub description: String,
    pub member_count: i64,
    pub organization_name: String,
}
