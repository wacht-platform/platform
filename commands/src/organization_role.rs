use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::OrganizationRole;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Serialize, Deserialize)]
pub struct CreateOrganizationRoleCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub name: String,
    pub permissions: Vec<String>,
}

impl CreateOrganizationRoleCommand {
    pub fn new(
        deployment_id: i64,
        organization_id: i64,
        name: String,
        permissions: Vec<String>,
    ) -> Self {
        Self {
            deployment_id,
            organization_id,
            name,
            permissions,
        }
    }
}

impl Command for CreateOrganizationRoleCommand {
    type Output = OrganizationRole;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Check if organization exists
        let org_exists = sqlx::query!(
            "SELECT id FROM organizations WHERE deployment_id = $1 AND id = $2",
            self.deployment_id,
            self.organization_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        if org_exists.is_none() {
            return Err(AppError::NotFound("Organization not found".to_string()));
        }

        // Check if role name already exists in this organization
        let existing_role = sqlx::query!(
            "SELECT id FROM organization_roles WHERE organization_id = $1 AND name = $2",
            self.organization_id,
            self.name
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        if existing_role.is_some() {
            return Err(AppError::BadRequest(
                "Role with this name already exists".to_string(),
            ));
        }

        // Create role with permissions stored as array
        let role = sqlx::query!(
            r#"
            INSERT INTO organization_roles (id, organization_id, deployment_id, name, permissions, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, permissions
            "#,
            app_state.sf.next_id()? as i64,
            self.organization_id,
            self.deployment_id,
            self.name,
            &self.permissions,
            chrono::Utc::now(),
            chrono::Utc::now()
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(OrganizationRole {
            id: role.id,
            created_at: role.created_at,
            updated_at: role.updated_at,
            name: self.name,
            permissions: role.permissions,
            is_deployment_level: false, // Organization-specific roles are never deployment-level
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct UpdateOrganizationRoleCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub role_id: i64,
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

impl UpdateOrganizationRoleCommand {
    pub fn new(
        deployment_id: i64,
        organization_id: i64,
        role_id: i64,
        name: Option<String>,
        permissions: Option<Vec<String>>,
    ) -> Self {
        Self {
            deployment_id,
            organization_id,
            role_id,
            name,
            permissions,
        }
    }
}

impl Command for UpdateOrganizationRoleCommand {
    type Output = OrganizationRole;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Check if role exists
        let role_exists = sqlx::query!(
            "SELECT id FROM organization_roles WHERE id = $1 AND organization_id = $2",
            self.role_id,
            self.organization_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        if role_exists.is_none() {
            return Err(AppError::NotFound(
                "Organization role not found".to_string(),
            ));
        }

        // Build update query dynamically
        let mut query_parts = Vec::new();
        let mut param_count = 2; // role_id is $1, updated_at will be the last param

        if self.name.is_some() {
            query_parts.push(format!("name = ${}", param_count));
            param_count += 1;
        }
        if self.permissions.is_some() {
            query_parts.push(format!("permissions = ${}", param_count));
            param_count += 1;
        }

        if query_parts.is_empty() {
            return Err(AppError::BadRequest("No fields to update".to_string()));
        }

        query_parts.push(format!("updated_at = ${}", param_count));

        let query_str = format!(
            "UPDATE organization_roles SET {} WHERE id = $1 RETURNING id, created_at, updated_at, name, permissions",
            query_parts.join(", ")
        );

        let mut query = sqlx::query(&query_str).bind(self.role_id);

        if let Some(name) = &self.name {
            query = query.bind(name);
        }
        if let Some(permissions) = &self.permissions {
            query = query.bind(permissions);
        }

        query = query.bind(chrono::Utc::now());

        let role = query.fetch_one(&app_state.db_pool).await?;

        // Get permissions from database
        let permissions_vec: Vec<String> = role.get("permissions");

        Ok(OrganizationRole {
            id: role.get("id"),
            created_at: role.get("created_at"),
            updated_at: role.get("updated_at"),
            name: role.get("name"),
            permissions: permissions_vec,
            is_deployment_level: false, // Organization-specific roles are never deployment-level
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct DeleteOrganizationRoleCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub role_id: i64,
}

impl DeleteOrganizationRoleCommand {
    pub fn new(deployment_id: i64, organization_id: i64, role_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
            role_id,
        }
    }
}

impl Command for DeleteOrganizationRoleCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Check if role exists
        let role_exists = sqlx::query!(
            "SELECT id FROM organization_roles WHERE id = $1 AND organization_id = $2",
            self.role_id,
            self.organization_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        if role_exists.is_none() {
            return Err(AppError::NotFound(
                "Organization role not found".to_string(),
            ));
        }

        // Delete role (this should cascade to permissions and role assignments)
        sqlx::query!("DELETE FROM organization_roles WHERE id = $1", self.role_id)
            .execute(&app_state.db_pool)
            .await?;

        Ok(())
    }
}
