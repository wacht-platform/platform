use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentWorkspaceRole {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub permissions: Vec<String>,
    pub organization_id: Option<i64>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub workspace_id: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentOrganizationRole {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub name: String,
    pub permissions: Vec<String>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub organization_id: Option<i64>,
}

impl DeploymentWorkspaceRole {
    pub fn admin() -> Self {
        Self {
            id: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            name: "Admin".to_string(),
            permissions: vec!["workspace:admin".to_string()],
            organization_id: None,
            deployment_id: 0,
            workspace_id: None,
        }
    }

    pub fn member() -> Self {
        Self {
            id: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            name: "Member".to_string(),
            permissions: vec!["workspace:member".to_string()],
            organization_id: None,
            deployment_id: 0,
            workspace_id: None,
        }
    }
}

impl DeploymentOrganizationRole {
    pub fn admin() -> Self {
        Self {
            id: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            name: "Admin".to_string(),
            permissions: vec!["organization:admin".to_string()],
            deployment_id: 0,
            organization_id: None,
        }
    }
}

impl DeploymentOrganizationRole {
    pub fn member() -> Self {
        Self {
            id: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            name: "Member".to_string(),
            permissions: vec!["organization:member".to_string()],
            deployment_id: 0,
            organization_id: None,
        }
    }
}
