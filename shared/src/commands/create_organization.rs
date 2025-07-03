use crate::{commands::Command, error::AppError, models::Organization, state::AppState};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
pub struct CreateOrganizationCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<Value>,
    pub private_metadata: Option<Value>,
}

impl CreateOrganizationCommand {
    pub fn new(
        deployment_id: i64,
        name: String,
        description: Option<String>,
        image_url: Option<String>,
        public_metadata: Option<Value>,
        private_metadata: Option<Value>,
    ) -> Self {
        Self {
            deployment_id,
            name,
            description,
            image_url,
            public_metadata,
            private_metadata,
        }
    }
}

impl Command for CreateOrganizationCommand {
    type Output = Organization;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let default_public_metadata = Value::Object(serde_json::Map::new());
        let default_private_metadata = Value::Object(serde_json::Map::new());

        let organization = sqlx::query!(
            r#"
            INSERT INTO organizations (
                id, deployment_id, name, description, image_url,
                public_metadata, private_metadata, member_count, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 0, $8, $9)
            RETURNING
                id, created_at, updated_at, deployment_id,
                name, description as "description?", image_url as "image_url?", member_count,
                public_metadata, private_metadata
            "#,
            app_state.sf.next_id()? as i64,
            self.deployment_id,
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
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(Organization {
            id: organization.id,
            created_at: organization.created_at,
            updated_at: organization.updated_at,
            name: organization.name,
            description: organization.description.unwrap_or_default(),
            image_url: organization.image_url.unwrap_or_default(),
            member_count: organization.member_count,
            public_metadata: organization.public_metadata,
            private_metadata: organization.private_metadata,
        })
    }
}
