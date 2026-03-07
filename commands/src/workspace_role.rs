use common::error::AppError;
use models::WorkspaceRole;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Serialize, Deserialize)]
pub struct CreateWorkspaceRoleCommand {
    pub role_id: Option<i64>,
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
            role_id: None,
            deployment_id,
            workspace_id,
            name,
            permissions,
        }
    }

    pub fn with_role_id(mut self, role_id: i64) -> Self {
        self.role_id = Some(role_id);
        self
    }
}

impl CreateWorkspaceRoleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<WorkspaceRole, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let role_id = self
            .role_id
            .ok_or_else(|| AppError::Validation("role_id is required".to_string()))?;
        let now = chrono::Utc::now();
        let row = sqlx::query!(
            r#"
            WITH dup AS (
                SELECT id
                FROM workspace_roles
                WHERE workspace_id = $2 AND name = $3
            ),
            ins AS (
                INSERT INTO workspace_roles (
                    id, workspace_id, deployment_id, name, permissions, created_at, updated_at
                )
                SELECT $1, $2, $4, $3, $5, $6, $7
                WHERE NOT EXISTS (SELECT 1 FROM dup)
                RETURNING id, created_at, updated_at, permissions
            )
            SELECT
                (SELECT EXISTS(SELECT 1 FROM dup)) AS "role_exists!",
                ins.id,
                ins.created_at,
                ins.updated_at,
                ins.permissions
            FROM ins
            UNION ALL
            SELECT
                (SELECT EXISTS(SELECT 1 FROM dup)) AS "role_exists!",
                NULL::BIGINT AS id,
                NULL::TIMESTAMPTZ AS created_at,
                NULL::TIMESTAMPTZ AS updated_at,
                NULL::TEXT[] AS permissions
            WHERE NOT EXISTS (SELECT 1 FROM ins)
            LIMIT 1
            "#,
            role_id,
            self.workspace_id,
            self.name,
            self.deployment_id,
            &self.permissions,
            now,
            now
        )
        .fetch_one(executor)
        .await?;

        if row.role_exists {
            return Err(AppError::BadRequest(
                "Role with this name already exists".to_string(),
            ));
        }

        Ok(WorkspaceRole {
            id: row.id.unwrap_or(role_id),
            created_at: row.created_at.unwrap_or(now),
            updated_at: row.updated_at.unwrap_or(now),
            name: self.name,
            permissions: row.permissions.unwrap_or_default(),
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

impl UpdateWorkspaceRoleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<WorkspaceRole, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        // Build update query dynamically
        let mut query_parts = Vec::new();
        let mut param_count = 3; // role_id is $1, workspace_id is $2

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
            "UPDATE workspace_roles SET {} WHERE id = $1 AND workspace_id = $2 RETURNING id, created_at, updated_at, name, permissions",
            query_parts.join(", ")
        );

        let mut query = sqlx::query(&query_str)
            .bind(self.role_id)
            .bind(self.workspace_id);

        if let Some(name) = &self.name {
            query = query.bind(name);
        }
        if let Some(permissions) = &self.permissions {
            query = query.bind(permissions);
        }

        query = query.bind(chrono::Utc::now());

        let role = query
            .fetch_optional(executor)
            .await?
            .ok_or_else(|| AppError::NotFound("Workspace role not found".to_string()))?;

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

impl DeleteWorkspaceRoleCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            DELETE FROM workspace_roles
            WHERE id = $1 AND workspace_id = $2
            "#,
            self.role_id,
            self.workspace_id
        )
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Workspace role not found".to_string()));
        }

        Ok(())
    }
}
