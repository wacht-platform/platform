use common::error::AppError;
use models::Workspace;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
pub struct CreateWorkspaceCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<Value>,
    pub private_metadata: Option<Value>,
}

impl CreateWorkspaceCommand {
    pub fn new(
        deployment_id: i64,
        organization_id: i64,
        name: String,
        description: Option<String>,
        image_url: Option<String>,
        public_metadata: Option<Value>,
        private_metadata: Option<Value>,
    ) -> Self {
        Self {
            deployment_id,
            organization_id,
            name,
            description,
            image_url,
            public_metadata,
            private_metadata,
        }
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        workspace_id: i64,
    ) -> Result<Workspace, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let default_public_metadata = Value::Object(serde_json::Map::new());
        let default_private_metadata = Value::Object(serde_json::Map::new());

        let workspace = sqlx::query!(
            r#"
            INSERT INTO workspaces (
                id, deployment_id, organization_id, name, description, image_url,
                public_metadata, private_metadata, member_count, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 0, $9, $10)
            RETURNING
                id, created_at, updated_at, deployment_id, organization_id,
                name, description as "description?", image_url as "image_url?", member_count,
                public_metadata, private_metadata
            "#,
            workspace_id,
            self.deployment_id,
            self.organization_id,
            self.name,
            self.description.as_deref().unwrap_or(""),
            self.image_url.as_deref().unwrap_or(""),
            self.public_metadata
                .as_ref()
                .unwrap_or(&default_public_metadata),
            self.private_metadata
                .as_ref()
                .unwrap_or(&default_private_metadata),
            chrono::Utc::now(),
            chrono::Utc::now(),
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(Workspace {
            id: workspace.id,
            created_at: workspace.created_at,
            updated_at: workspace.updated_at,
            name: workspace.name,
            description: workspace.description.unwrap_or_default(),
            image_url: workspace.image_url.unwrap_or_default(),
            member_count: workspace.member_count,
            public_metadata: workspace.public_metadata,
            private_metadata: workspace.private_metadata,
        })
    }
}
