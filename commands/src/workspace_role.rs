use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::WorkspaceRole;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Serialize, Deserialize)]
pub struct CreateWorkspaceRoleCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub name: String,
    pub permissions: Vec<String>,
}

impl CreateWorkspaceRoleCommand {
    pub fn new(
        deployment_id: i64,
        workspace_id: i64,
        name: String,
        permissions: Vec<String>,
    ) -> Self {
        Self {
            deployment_id,
            workspace_id,
            name,
            permissions,
        }
    }
}

impl Command for CreateWorkspaceRoleCommand {
    type Output = WorkspaceRole;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer(), app_state.sf.next_id()? as i64)
            .await
    }
}

impl CreateWorkspaceRoleCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        role_id: i64,
    ) -> Result<WorkspaceRole, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        // Check if role with same name already exists in this workspace
        let existing_role = sqlx::query!(
            "SELECT id FROM workspace_roles WHERE workspace_id = $1 AND name = $2",
            self.workspace_id,
            self.name
        )
        .fetch_optional(&mut *conn)
        .await?;

        if existing_role.is_some() {
            return Err(AppError::BadRequest(
                "Role with this name already exists".to_string(),
            ));
        }

        // Create role with permissions stored as array
        let role = sqlx::query!(
            r#"
            INSERT INTO workspace_roles (id, workspace_id, deployment_id, name, permissions, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, permissions
            "#,
            role_id,
            self.workspace_id,
            self.deployment_id,
            self.name,
            &self.permissions,
            chrono::Utc::now(),
            chrono::Utc::now()
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(WorkspaceRole {
            id: role.id,
            created_at: role.created_at,
            updated_at: role.updated_at,
            name: self.name,
            permissions: role.permissions,
            is_deployment_level: false, // Workspace-specific roles are never deployment-level
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct UpdateWorkspaceRoleCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub role_id: i64,
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

impl UpdateWorkspaceRoleCommand {
    pub fn new(
        deployment_id: i64,
        workspace_id: i64,
        role_id: i64,
        name: Option<String>,
        permissions: Option<Vec<String>>,
    ) -> Self {
        Self {
            deployment_id,
            workspace_id,
            role_id,
            name,
            permissions,
        }
    }
}

impl Command for UpdateWorkspaceRoleCommand {
    type Output = WorkspaceRole;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl UpdateWorkspaceRoleCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<WorkspaceRole, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        // Check if role exists
        let role_exists = sqlx::query!(
            "SELECT id FROM workspace_roles WHERE id = $1 AND workspace_id = $2",
            self.role_id,
            self.workspace_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        if role_exists.is_none() {
            return Err(AppError::NotFound("Workspace role not found".to_string()));
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
            "UPDATE workspace_roles SET {} WHERE id = $1 RETURNING id, created_at, updated_at, name, permissions",
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

        let role = query.fetch_one(&mut *conn).await?;

        // Get permissions from database
        let permissions_vec: Vec<String> = role.get("permissions");

        Ok(WorkspaceRole {
            id: role.get("id"),
            created_at: role.get("created_at"),
            updated_at: role.get("updated_at"),
            name: role.get("name"),
            permissions: permissions_vec,
            is_deployment_level: false, // Workspace-specific roles are never deployment-level
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct DeleteWorkspaceRoleCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub role_id: i64,
}

impl DeleteWorkspaceRoleCommand {
    pub fn new(deployment_id: i64, workspace_id: i64, role_id: i64) -> Self {
        Self {
            deployment_id,
            workspace_id,
            role_id,
        }
    }
}

impl Command for DeleteWorkspaceRoleCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl DeleteWorkspaceRoleCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        // Check if role exists
        let role_exists = sqlx::query!(
            "SELECT id FROM workspace_roles WHERE id = $1 AND workspace_id = $2",
            self.role_id,
            self.workspace_id
        )
        .fetch_optional(&mut *conn)
        .await?;

        if role_exists.is_none() {
            return Err(AppError::NotFound("Workspace role not found".to_string()));
        }

        // Delete role (this should cascade to permissions and role assignments)
        sqlx::query!("DELETE FROM workspace_roles WHERE id = $1", self.role_id)
            .execute(&mut *conn)
            .await?;

        Ok(())
    }
}
