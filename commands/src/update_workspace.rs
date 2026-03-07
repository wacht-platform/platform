use common::error::AppError;
use models::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

#[derive(Serialize, Deserialize)]
pub struct UpdateWorkspaceCommand {
    pub deployment_id: i64,
    pub workspace_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<Value>,
    pub private_metadata: Option<Value>,
}

impl UpdateWorkspaceCommand {
    pub fn new(deployment_id: i64, workspace_id: i64) -> Self {
        Self {
            deployment_id,
            workspace_id,
            name: None,
            description: None,
            image_url: None,
            public_metadata: None,
            private_metadata: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_image_url(mut self, image_url: Option<String>) -> Self {
        self.image_url = image_url;
        self
    }

    pub fn with_public_metadata(mut self, public_metadata: Value) -> Self {
        self.public_metadata = Some(public_metadata);
        self
    }

    pub fn with_private_metadata(mut self, private_metadata: Value) -> Self {
        self.private_metadata = Some(private_metadata);
        self
    }

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<Workspace, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut query_parts = Vec::new();
        let mut param_count = 3; // deployment_id and workspace_id are $1 and $2

        if self.name.is_some() {
            query_parts.push(format!("name = ${}", param_count));
            param_count += 1;
        }
        if self.description.is_some() {
            query_parts.push(format!("description = ${}", param_count));
            param_count += 1;
        }
        if self.image_url.is_some() {
            query_parts.push(format!("image_url = ${}", param_count));
            param_count += 1;
        }
        if self.public_metadata.is_some() {
            query_parts.push(format!("public_metadata = ${}", param_count));
            param_count += 1;
        }
        if self.private_metadata.is_some() {
            query_parts.push(format!("private_metadata = ${}", param_count));
            param_count += 1;
        }

        if query_parts.is_empty() {
            return Err(AppError::BadRequest("No fields to update".to_string()));
        }

        query_parts.push(format!("updated_at = ${}", param_count));

        let query_str = format!(
            r#"
            UPDATE workspaces
            SET {}
            WHERE id = $2 AND organization_id IN (SELECT id FROM organizations WHERE deployment_id = $1)
            RETURNING
                id, created_at, updated_at,
                name, description, image_url, member_count,
                public_metadata, private_metadata
            "#,
            query_parts.join(", ")
        );

        let mut query = sqlx::query(&query_str)
            .bind(self.deployment_id)
            .bind(self.workspace_id);

        if let Some(name) = &self.name {
            query = query.bind(name);
        }
        if let Some(description) = &self.description {
            query = query.bind(description);
        }
        if let Some(image_url) = &self.image_url {
            query = query.bind(image_url);
        }
        if let Some(public_metadata) = &self.public_metadata {
            query = query.bind(public_metadata);
        }
        if let Some(private_metadata) = &self.private_metadata {
            query = query.bind(private_metadata);
        }

        query = query.bind(chrono::Utc::now());

        let row = query.fetch_one(&mut *conn).await.map_err(|e| match e {
            sqlx::Error::RowNotFound => AppError::NotFound("Workspace not found".to_string()),
            _ => AppError::Database(e),
        })?;

        Ok(Workspace {
            id: row.get("id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            name: row.get("name"),
            description: row.get("description"),
            image_url: row.get("image_url"),
            member_count: row.get("member_count"),
            public_metadata: row.get("public_metadata"),
            private_metadata: row.get("private_metadata"),
        })
    }
}
