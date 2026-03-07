use common::error::AppError;
use models::Organization;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::Row;

#[derive(Serialize, Deserialize)]
pub struct UpdateOrganizationCommand {
    pub deployment_id: i64,
    pub organization_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub public_metadata: Option<Value>,
    pub private_metadata: Option<Value>,
}

impl UpdateOrganizationCommand {
    pub fn new(
        deployment_id: i64,
        organization_id: i64,
        name: Option<String>,
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

    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<Organization, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let mut query_parts = Vec::new();
        let mut param_count = 3; // deployment_id and organization_id are $1 and $2

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
            UPDATE organizations
            SET {}
            WHERE deployment_id = $1 AND id = $2
            RETURNING
                id, created_at, updated_at, deployment_id,
                name, description, image_url, member_count,
                public_metadata, private_metadata
            "#,
            query_parts.join(", ")
        );

        let mut query = sqlx::query(&query_str)
            .bind(self.deployment_id)
            .bind(self.organization_id);

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

        let organization = query.fetch_one(&mut *conn).await?;

        Ok(Organization {
            id: organization.get("id"),
            created_at: organization.get("created_at"),
            updated_at: organization.get("updated_at"),
            name: organization.get("name"),
            description: organization.get("description"),
            image_url: organization.get("image_url"),
            member_count: organization.get("member_count"),
            public_metadata: organization.get("public_metadata"),
            private_metadata: organization.get("private_metadata"),
        })
    }
}
