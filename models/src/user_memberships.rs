use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Organization, OrganizationRole, Workspace, WorkspaceRole};

#[derive(Serialize, Deserialize, Clone)]
pub struct UserOrganizationMembership {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub organization_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub user_id: i64,
    pub public_metadata: Value,
    pub roles: Vec<OrganizationRole>,
    pub organization: Organization,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserWorkspaceMembership {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub workspace_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub organization_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub organization_membership_id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub user_id: i64,
    pub public_metadata: Value,
    pub roles: Vec<WorkspaceRole>,
    pub workspace: Workspace,
}
