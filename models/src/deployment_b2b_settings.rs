use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{DeploymentOrganizationRole, DeploymentWorkspaceRole};

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentPermissionCatalogEntry {
    pub key: String,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentB2bSettings {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub deployment_id: i64,
    pub organizations_enabled: bool,
    pub workspaces_enabled: bool,
    pub ip_allowlist_per_org_enabled: bool,
    pub allow_users_to_create_orgs: bool,
    pub max_orgs_per_user: i32,
    pub max_allowed_org_members: i64,
    pub max_allowed_workspace_members: i64,
    pub allow_org_deletion: bool,
    pub allow_workspace_deletion: bool,
    pub custom_org_role_enabled: bool,
    pub limit_org_creation_per_user: bool,
    pub limit_workspace_creation_per_org: bool,
    pub org_creation_per_user_count: i32,
    pub workspaces_per_org_count: i32,
    pub custom_workspace_role_enabled: bool,
    pub default_workspace_creator_role_id: i64,
    pub default_workspace_member_role_id: i64,
    pub default_org_creator_role_id: i64,
    pub default_org_member_role_id: i64,
    pub workspace_permissions: Option<Vec<String>>,
    pub organization_permissions: Option<Vec<String>>,
    pub workspace_permission_catalog: Option<Vec<DeploymentPermissionCatalogEntry>>,
    pub organization_permission_catalog: Option<Vec<DeploymentPermissionCatalogEntry>>,
    pub ip_allowlist_per_workspace_enabled: bool,
    pub enforce_mfa_per_org_enabled: bool,
    pub enforce_mfa_per_workspace_enabled: bool,
    pub enterprise_sso_enabled: bool,
}

impl Default for DeploymentB2bSettings {
    fn default() -> Self {
        Self {
            id: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deployment_id: 0,
            organizations_enabled: false,
            workspaces_enabled: false,
            ip_allowlist_per_org_enabled: false,
            max_allowed_org_members: 100,
            max_allowed_workspace_members: 100,
            allow_org_deletion: false,
            allow_workspace_deletion: false,
            limit_org_creation_per_user: false,
            limit_workspace_creation_per_org: false,
            org_creation_per_user_count: 0,
            workspaces_per_org_count: 0,
            custom_org_role_enabled: false,
            custom_workspace_role_enabled: false,
            default_workspace_creator_role_id: 0,
            default_workspace_member_role_id: 0,
            default_org_creator_role_id: 0,
            default_org_member_role_id: 0,
            allow_users_to_create_orgs: true,
            max_orgs_per_user: 0,
            workspace_permissions: None,
            organization_permissions: None,
            workspace_permission_catalog: None,
            organization_permission_catalog: None,
            ip_allowlist_per_workspace_enabled: false,
            enforce_mfa_per_org_enabled: false,
            enforce_mfa_per_workspace_enabled: false,
            enterprise_sso_enabled: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeploymentB2bSettingsWithRoles {
    #[serde(flatten)]
    pub settings: DeploymentB2bSettings,
    pub default_workspace_creator_role: DeploymentWorkspaceRole,
    pub default_workspace_member_role: DeploymentWorkspaceRole,
    pub default_org_creator_role: DeploymentOrganizationRole,
    pub default_org_member_role: DeploymentOrganizationRole,
}
