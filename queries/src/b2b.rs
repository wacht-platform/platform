use sqlx::{Row, query, query_as};

use crate::prelude::*;
use models::{
    DeploymentOrganizationRole, DeploymentWorkspaceRole, Organization, OrganizationDetails,
    OrganizationMemberDetails, OrganizationRole, Workspace, WorkspaceDetails,
    WorkspaceMemberDetails, WorkspaceRole, WorkspaceWithOrganizationName,
};

pub struct GetDeploymentWorkspaceRolesQuery {
    deployment_id: i64,
}

impl GetDeploymentWorkspaceRolesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentWorkspaceRolesQuery {
    type Output = Vec<DeploymentWorkspaceRole>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = query_as!(
            DeploymentWorkspaceRole,
            r#"
            SELECT * FROM workspace_roles WHERE deployment_id = $1"#,
            self.deployment_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(rows)
    }
}

pub struct GetDeploymentOrganizationRolesQuery {
    deployment_id: i64,
}

impl GetDeploymentOrganizationRolesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Query for GetDeploymentOrganizationRolesQuery {
    type Output = Vec<DeploymentOrganizationRole>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let rows = query_as!(
            DeploymentOrganizationRole,
            r#"SELECT * FROM organization_roles WHERE deployment_id = $1"#,
            self.deployment_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        Ok(rows)
    }
}

pub struct DeploymentOrganizationListQuery {
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
    deployment_id: i64,
}

impl DeploymentOrganizationListQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
        }
    }

    pub fn offset(&self, offset: i64) -> Self {
        Self {
            offset,
            sort_key: self.sort_key.clone(),
            sort_order: self.sort_order.clone(),
            limit: self.limit,
            deployment_id: self.deployment_id,
        }
    }

    pub fn limit(&self, limit: i32) -> Self {
        Self {
            offset: self.offset,
            sort_key: self.sort_key.clone(),
            sort_order: self.sort_order.clone(),
            limit,
            deployment_id: self.deployment_id,
        }
    }

    pub fn sort_key(&self, sort_key: Option<String>) -> Self {
        Self {
            offset: self.offset,
            sort_key,
            sort_order: self.sort_order.clone(),
            limit: self.limit,
            deployment_id: self.deployment_id,
        }
    }

    pub fn sort_order(&self, sort_order: Option<String>) -> Self {
        Self {
            offset: self.offset,
            sort_key: self.sort_key.clone(),
            sort_order,
            limit: self.limit,
            deployment_id: self.deployment_id,
        }
    }
}

impl Query for DeploymentOrganizationListQuery {
    type Output = Vec<Organization>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_str = String::from(
            r#"
            SELECT
                o.id, o.created_at, o.updated_at,
                o.name, o.image_url, o.description, o.member_count,
                o.public_metadata, o.private_metadata
            FROM organizations o
            WHERE o.deployment_id = $1
            "#,
        );

        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");
        query_str.push_str(&format!(" ORDER BY o.{} {}", sort_key, sort_order));

        query_str.push_str(" OFFSET $2 LIMIT $3");

        let rows = query(&query_str)
            .bind(self.deployment_id)
            .bind(self.offset)
            .bind(self.limit)
            .fetch_all(&app_state.db_pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| Organization {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                name: row.get("name"),
                image_url: row.get("image_url"),
                description: row.get("description"),
                member_count: row.get("member_count"),
                public_metadata: row.get("public_metadata"),
                private_metadata: row.get("private_metadata"),
            })
            .collect())
    }
}

pub struct DeploymentWorkspaceListQuery {
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
    deployment_id: i64,
}

impl DeploymentWorkspaceListQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
        }
    }

    pub fn offset(&self, offset: i64) -> Self {
        Self {
            offset,
            sort_key: self.sort_key.clone(),
            sort_order: self.sort_order.clone(),
            limit: self.limit,
            deployment_id: self.deployment_id,
        }
    }

    pub fn limit(&self, limit: i32) -> Self {
        Self {
            offset: self.offset,
            sort_key: self.sort_key.clone(),
            sort_order: self.sort_order.clone(),
            limit,
            deployment_id: self.deployment_id,
        }
    }

    pub fn sort_key(&self, sort_key: Option<String>) -> Self {
        Self {
            offset: self.offset,
            sort_key,
            sort_order: self.sort_order.clone(),
            limit: self.limit,
            deployment_id: self.deployment_id,
        }
    }

    pub fn sort_order(&self, sort_order: Option<String>) -> Self {
        Self {
            offset: self.offset,
            sort_key: self.sort_key.clone(),
            sort_order,
            limit: self.limit,
            deployment_id: self.deployment_id,
        }
    }
}

impl Query for DeploymentWorkspaceListQuery {
    type Output = Vec<WorkspaceWithOrganizationName>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_str = String::from(
            r#"
            SELECT
                w.id, w.created_at, w.updated_at, w.deleted_at,
                w.name, w.image_url, w.description, w.member_count,
                w.public_metadata, w.private_metadata,
                o.name AS organization_name
            FROM workspaces w
            LEFT JOIN organizations o ON w.organization_id = o.id
            WHERE w.deployment_id = $1
            "#,
        );

        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");
        query_str.push_str(&format!(" ORDER BY o.{} {}", sort_key, sort_order));

        query_str.push_str(" OFFSET $2 LIMIT $3");

        let rows = query(&query_str)
            .bind(self.deployment_id)
            .bind(self.offset)
            .bind(self.limit)
            .fetch_all(&app_state.db_pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| WorkspaceWithOrganizationName {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                name: row.get("name"),
                image_url: row.get("image_url"),
                description: row.get("description"),
                member_count: row.get("member_count"),
                organization_name: row.get("organization_name"),
            })
            .collect())
    }
}

pub struct GetOrganizationDetailsQuery {
    deployment_id: i64,
    organization_id: i64,
}

impl GetOrganizationDetailsQuery {
    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
        }
    }
}

impl Query for GetOrganizationDetailsQuery {
    type Output = OrganizationDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get organization basic info
        let org_row = sqlx::query!(
            r#"
            SELECT
                o.id, o.created_at, o.updated_at,
                o.name, o.image_url, o.description, o.member_count,
                o.public_metadata, o.private_metadata
            FROM organizations o
            WHERE o.deployment_id = $1 AND o.id = $2
            "#,
            self.deployment_id,
            self.organization_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        // Get organization members with user details
        let member_rows = sqlx::query!(
            r#"
            SELECT
                om.id, om.created_at, om.updated_at,
                om.organization_id, om.user_id,
                u.first_name, u.last_name, u.username,
                u.created_at as user_created_at,
                e.email_address as "primary_email_address?",
                p.phone_number as "primary_phone_number?"
            FROM organization_memberships om
            JOIN users u ON om.user_id = u.id
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            WHERE om.organization_id = $1
            "#,
            self.organization_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        // Get organization roles with permissions (both deployment-level and organization-specific)
        let role_rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, name, permissions, organization_id
            FROM organization_roles
            WHERE (deployment_id = $1 AND organization_id IS NULL)
               OR organization_id = $2
            "#,
            self.deployment_id,
            self.organization_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let roles: Vec<OrganizationRole> = role_rows
            .into_iter()
            .map(|row| OrganizationRole {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                name: row.name,
                permissions: row.permissions,
                is_deployment_level: row.organization_id.is_none(),
            })
            .collect();

        // Build member details (simplified - in real implementation, you'd need to join with role assignments)
        let members: Vec<OrganizationMemberDetails> = member_rows
            .into_iter()
            .map(|row| OrganizationMemberDetails {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                organization_id: row.organization_id,
                user_id: row.user_id,
                roles: vec![], // Simplified for now - would need async context to fetch roles
                first_name: row.first_name,
                last_name: row.last_name,
                username: if row.username.is_empty() {
                    None
                } else {
                    Some(row.username)
                },
                primary_email_address: row.primary_email_address,
                primary_phone_number: row.primary_phone_number,
                user_created_at: row.user_created_at,
            })
            .collect();

        // Get organization workspaces
        let workspace_rows = sqlx::query!(
            r#"
            SELECT
                id, created_at, updated_at,
                name, image_url as "image_url?", description as "description?", member_count,
                public_metadata, private_metadata
            FROM workspaces
            WHERE organization_id = $1
            ORDER BY created_at DESC
            "#,
            self.organization_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let workspaces: Vec<Workspace> = workspace_rows
            .into_iter()
            .map(|row| Workspace {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                name: row.name,
                image_url: row.image_url.unwrap_or_default(),
                description: row.description.unwrap_or_default(),
                member_count: row.member_count,
                public_metadata: row.public_metadata,
                private_metadata: row.private_metadata,
            })
            .collect();

        Ok(OrganizationDetails {
            id: org_row.id,
            created_at: org_row.created_at,
            updated_at: org_row.updated_at,
            name: org_row.name,
            image_url: org_row.image_url,
            description: org_row.description.unwrap_or_default(),
            member_count: org_row.member_count,
            public_metadata: org_row.public_metadata,
            private_metadata: org_row.private_metadata,
            members,
            roles,
            workspaces,
        })
    }
}

pub struct GetWorkspaceDetailsQuery {
    deployment_id: i64,
    workspace_id: i64,
}

impl GetWorkspaceDetailsQuery {
    pub fn new(deployment_id: i64, workspace_id: i64) -> Self {
        Self {
            deployment_id,
            workspace_id,
        }
    }
}

impl Query for GetWorkspaceDetailsQuery {
    type Output = WorkspaceDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get workspace basic info with organization name
        let workspace_row = sqlx::query!(
            r#"
            SELECT
                w.id, w.created_at, w.updated_at,
                w.name, w.image_url, w.description, w.member_count,
                w.public_metadata, w.private_metadata, w.organization_id,
                o.name as "organization_name?"
            FROM workspaces w
            LEFT JOIN organizations o ON w.organization_id = o.id
            WHERE w.deployment_id = $1 AND w.id = $2
            "#,
            self.deployment_id,
            self.workspace_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        // Get workspace members with user details
        let member_rows = sqlx::query!(
            r#"
            SELECT
                wm.id, wm.created_at, wm.updated_at,
                wm.workspace_id, wm.user_id,
                u.first_name, u.last_name, u.username,
                u.created_at as user_created_at,
                e.email_address as "primary_email_address?",
                p.phone_number as "primary_phone_number?"
            FROM workspace_memberships wm
            JOIN users u ON wm.user_id = u.id
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            WHERE wm.workspace_id = $1
            "#,
            self.workspace_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        // Get workspace roles with permissions (both deployment-level and workspace-specific)
        let role_rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, name, permissions, workspace_id
            FROM workspace_roles
            WHERE (deployment_id = $1 AND workspace_id IS NULL)
               OR workspace_id = $2
            "#,
            self.deployment_id,
            self.workspace_id
        )
        .fetch_all(&app_state.db_pool)
        .await?;

        let roles: Vec<WorkspaceRole> = role_rows
            .into_iter()
            .map(|row| WorkspaceRole {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                name: row.name,
                permissions: row.permissions,
                is_deployment_level: row.workspace_id.is_none(),
            })
            .collect();

        let members: Vec<WorkspaceMemberDetails> = member_rows
            .into_iter()
            .map(|row| WorkspaceMemberDetails {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                workspace_id: row.workspace_id,
                user_id: row.user_id,
                roles: vec![],
                first_name: row.first_name,
                last_name: row.last_name,
                username: if row.username.is_empty() {
                    None
                } else {
                    Some(row.username)
                },
                primary_email_address: row.primary_email_address,
                primary_phone_number: row.primary_phone_number,
                user_created_at: row.user_created_at,
            })
            .collect();

        Ok(WorkspaceDetails {
            id: workspace_row.id,
            created_at: workspace_row.created_at,
            updated_at: workspace_row.updated_at,
            name: workspace_row.name,
            image_url: workspace_row.image_url,
            description: workspace_row.description,
            member_count: workspace_row.member_count as i32,
            public_metadata: workspace_row.public_metadata,
            private_metadata: workspace_row.private_metadata,
            organization_id: workspace_row.organization_id,
            organization_name: workspace_row.organization_name.unwrap_or_default(),
            members,
            roles,
        })
    }
}
