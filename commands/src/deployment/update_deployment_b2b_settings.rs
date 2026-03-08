use std::collections::HashSet;

use common::{HasDbRouter, HasRedisProvider, error::AppError};
use dto::json::DeploymentB2bSettingsUpdates;
use models::DeploymentPermissionCatalogEntry;
use serde_json::Value;

use super::ClearDeploymentCacheCommand;

pub struct UpdateDeploymentB2bSettingsCommand {
    deployment_id: i64,
    settings: DeploymentB2bSettingsUpdates,
}

impl UpdateDeploymentB2bSettingsCommand {
    pub fn new(deployment_id: i64, settings: DeploymentB2bSettingsUpdates) -> Self {
        Self {
            deployment_id,
            settings,
        }
    }
}

fn normalize_permission_catalog(
    entries: Vec<DeploymentPermissionCatalogEntry>,
) -> Result<Vec<DeploymentPermissionCatalogEntry>, AppError> {
    let mut seen = HashSet::<String>::new();
    let mut normalized = Vec::with_capacity(entries.len());

    for mut entry in entries {
        entry.key = entry.key.trim().to_string();
        if entry.key.is_empty() {
            return Err(AppError::BadRequest(
                "Permission key cannot be empty".to_string(),
            ));
        }
        if !seen.insert(entry.key.clone()) {
            return Err(AppError::BadRequest(format!(
                "Duplicate permission key in catalog: {}",
                entry.key
            )));
        }
        normalized.push(entry);
    }

    Ok(normalized)
}

fn active_permissions_from_catalog(entries: &[DeploymentPermissionCatalogEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|entry| !entry.archived)
        .map(|entry| entry.key.clone())
        .collect()
}

impl UpdateDeploymentB2bSettingsCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasRedisProvider,
    {
        let writer = deps.db_router().writer();
        let existing_catalogs = sqlx::query_as::<_, (Option<Value>, Option<Value>)>(
            "SELECT workspace_permission_catalog, organization_permission_catalog
             FROM deployment_b2b_settings
             WHERE deployment_id = $1",
        )
        .bind(self.deployment_id)
        .fetch_optional(writer)
        .await?;

        let (existing_workspace_catalog, existing_organization_catalog) = existing_catalogs
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "B2B settings for deployment {} not found",
                    self.deployment_id
                ))
            })?;

        let existing_workspace_catalog = existing_workspace_catalog
            .map(serde_json::from_value::<Vec<DeploymentPermissionCatalogEntry>>)
            .transpose()
            .map_err(|_| {
                AppError::BadRequest("Invalid workspace permission catalog".to_string())
            })?;

        let existing_organization_catalog = existing_organization_catalog
            .map(serde_json::from_value::<Vec<DeploymentPermissionCatalogEntry>>)
            .transpose()
            .map_err(|_| {
                AppError::BadRequest("Invalid organization permission catalog".to_string())
            })?;

        let mut query_builder =
            sqlx::QueryBuilder::new("UPDATE deployment_b2b_settings SET updated_at = NOW() ");

        if let Some(organizations_enabled) = self.settings.organizations_enabled {
            query_builder.push(", organizations_enabled = ");
            query_builder.push_bind(organizations_enabled);
        }

        if let Some(workspaces_enabled) = self.settings.workspaces_enabled {
            query_builder.push(", workspaces_enabled = ");
            query_builder.push_bind(workspaces_enabled);
        }

        if let Some(ip_allowlist_per_org_enabled) = self.settings.ip_allowlist_per_org_enabled {
            query_builder.push(", ip_allowlist_per_org_enabled = ");
            query_builder.push_bind(ip_allowlist_per_org_enabled);
        }

        if let Some(max_allowed_org_members) = self.settings.max_allowed_org_members {
            query_builder.push(", max_allowed_org_members = ");
            query_builder.push_bind(max_allowed_org_members);
        }

        if let Some(max_allowed_workspace_members) = self.settings.max_allowed_workspace_members {
            query_builder.push(", max_allowed_workspace_members = ");
            query_builder.push_bind(max_allowed_workspace_members);
        }

        if let Some(allow_org_deletion) = self.settings.allow_org_deletion {
            query_builder.push(", allow_org_deletion = ");
            query_builder.push_bind(allow_org_deletion);
        }

        if let Some(allow_workspace_deletion) = self.settings.allow_workspace_deletion {
            query_builder.push(", allow_workspace_deletion = ");
            query_builder.push_bind(allow_workspace_deletion);
        }

        if let Some(custom_org_role_enabled) = self.settings.custom_org_role_enabled {
            query_builder.push(", custom_org_role_enabled = ");
            query_builder.push_bind(custom_org_role_enabled);
        }

        if let Some(custom_workspace_role_enabled) = self.settings.custom_workspace_role_enabled {
            query_builder.push(", custom_workspace_role_enabled = ");
            query_builder.push_bind(custom_workspace_role_enabled);
        }

        if let Some(default_workspace_creator_role_id) =
            self.settings.default_workspace_creator_role_id
        {
            query_builder.push(", default_workspace_creator_role_id = ");
            query_builder.push_bind(default_workspace_creator_role_id);
        }

        if let Some(default_workspace_member_role_id) =
            self.settings.default_workspace_member_role_id
        {
            query_builder.push(", default_workspace_member_role_id = ");
            query_builder.push_bind(default_workspace_member_role_id);
        }

        if let Some(default_org_creator_role_id) = self.settings.default_org_creator_role_id {
            query_builder.push(", default_org_creator_role_id = ");
            query_builder.push_bind(default_org_creator_role_id);
        }

        if let Some(default_org_member_role_id) = self.settings.default_org_member_role_id {
            query_builder.push(", default_org_member_role_id = ");
            query_builder.push_bind(default_org_member_role_id);
        }

        if let Some(limit_org_creation_per_user) = self.settings.limit_org_creation_per_user {
            query_builder.push(", limit_org_creation_per_user = ");
            query_builder.push_bind(limit_org_creation_per_user);
        }

        if let Some(allow_users_to_create_orgs) = self.settings.allow_users_to_create_orgs {
            query_builder.push(", allow_users_to_create_orgs = ");
            query_builder.push_bind(allow_users_to_create_orgs);
        }

        if let Some(limit_workspace_creation_per_org) =
            self.settings.limit_workspace_creation_per_org
        {
            query_builder.push(", limit_workspace_creation_per_org = ");
            query_builder.push_bind(limit_workspace_creation_per_org);
        }

        if let Some(org_creation_per_user_count) = self.settings.org_creation_per_user_count {
            query_builder.push(", org_creation_per_user_count = ");
            query_builder.push_bind(org_creation_per_user_count);
        }

        if let Some(workspaces_per_org_count) = self.settings.workspaces_per_org_count {
            query_builder.push(", workspaces_per_org_count = ");
            query_builder.push_bind(workspaces_per_org_count);
        }

        let workspace_catalog =
            if let Some(workspace_catalog) = self.settings.workspace_permission_catalog {
                let normalized = normalize_permission_catalog(workspace_catalog)?;
                query_builder.push(", workspace_permission_catalog = ");
                query_builder.push_bind(serde_json::to_value(&normalized)?);
                Some(normalized)
            } else {
                existing_workspace_catalog
            };

        let organization_catalog =
            if let Some(organization_catalog) = self.settings.organization_permission_catalog {
                let normalized = normalize_permission_catalog(organization_catalog)?;
                query_builder.push(", organization_permission_catalog = ");
                query_builder.push_bind(serde_json::to_value(&normalized)?);
                Some(normalized)
            } else {
                existing_organization_catalog
            };

        if let Some(workspace_catalog) = workspace_catalog.as_ref() {
            query_builder.push(", workspace_permissions = ");
            query_builder.push_bind(active_permissions_from_catalog(workspace_catalog));
        } else if let Some(workspace_permissions) = self.settings.workspace_permissions {
            query_builder.push(", workspace_permissions = ");
            query_builder.push_bind(workspace_permissions);
        }

        if let Some(organization_catalog) = organization_catalog.as_ref() {
            query_builder.push(", organization_permissions = ");
            query_builder.push_bind(active_permissions_from_catalog(organization_catalog));
        } else if let Some(organization_permissions) = self.settings.organization_permissions {
            query_builder.push(", organization_permissions = ");
            query_builder.push_bind(organization_permissions);
        }

        if let Some(ip_allowlist_per_workspace_enabled) =
            self.settings.ip_allowlist_per_workspace_enabled
        {
            query_builder.push(", ip_allowlist_per_workspace_enabled = ");
            query_builder.push_bind(ip_allowlist_per_workspace_enabled);
        }

        if let Some(enforce_mfa_per_org_enabled) = self.settings.enforce_mfa_per_org_enabled {
            query_builder.push(", enforce_mfa_per_org_enabled = ");
            query_builder.push_bind(enforce_mfa_per_org_enabled);
        }

        if let Some(enforce_mfa_per_workspace_enabled) =
            self.settings.enforce_mfa_per_workspace_enabled
        {
            query_builder.push(", enforce_mfa_per_workspace_enabled = ");
            query_builder.push_bind(enforce_mfa_per_workspace_enabled);
        }

        if let Some(enterprise_sso_enabled) = self.settings.enterprise_sso_enabled {
            query_builder.push(", enterprise_sso_enabled = ");
            query_builder.push_bind(enterprise_sso_enabled);
        }

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);

        let result = query_builder.build().execute(writer).await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(format!(
                "B2B settings for deployment {} not found",
                self.deployment_id
            )));
        }

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;

        Ok(())
    }
}
