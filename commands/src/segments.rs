use common::error::AppError;
use models::Segment;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Serialize, Deserialize)]
pub struct CreateSegmentCommand {
    deployment_id: i64,
    name: String,
    r#type: String,
}

#[derive(Default)]
pub struct CreateSegmentCommandBuilder {
    deployment_id: Option<i64>,
    name: Option<String>,
    r#type: Option<String>,
}

impl CreateSegmentCommand {
    pub fn builder() -> CreateSegmentCommandBuilder {
        CreateSegmentCommandBuilder::default()
    }
}

impl CreateSegmentCommand {
    pub async fn execute_with(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
        id: i64,
    ) -> Result<Segment, AppError> {
        let mut conn = acquirer.acquire().await?;
        let segment = sqlx::query_as::<_, Segment>(
            r#"
            INSERT INTO segments (id, deployment_id, name, type)
            VALUES ($1, $2, $3, $4)
            RETURNING *
            "#,
        )
        .bind(id)
        .bind(self.deployment_id)
        .bind(self.name)
        .bind(self.r#type)
        .fetch_one(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        Ok(segment)
    }
}

impl CreateSegmentCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn segment_type(mut self, segment_type: String) -> Self {
        self.r#type = Some(segment_type);
        self
    }

    pub fn build(self) -> Result<CreateSegmentCommand, AppError> {
        Ok(CreateSegmentCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            name: self
                .name
                .ok_or_else(|| AppError::Validation("name is required".into()))?,
            r#type: self
                .r#type
                .ok_or_else(|| AppError::Validation("type is required".into()))?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct UpdateSegmentCommand {
    id: i64,
    deployment_id: i64,
    name: Option<String>,
}

#[derive(Default)]
pub struct UpdateSegmentCommandBuilder {
    id: Option<i64>,
    deployment_id: Option<i64>,
    name: Option<String>,
}

impl UpdateSegmentCommand {
    pub fn builder() -> UpdateSegmentCommandBuilder {
        UpdateSegmentCommandBuilder::default()
    }
}

impl UpdateSegmentCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<Segment, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let existing = sqlx::query(
            "SELECT id FROM segments WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.id)
        .bind(self.deployment_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        if existing.is_none() {
            return Err(AppError::NotFound("Segment not found".into()));
        }

        let mut query_builder = sqlx::QueryBuilder::new("UPDATE segments SET updated_at = NOW()");

        if let Some(name) = self.name {
            query_builder.push(", name = ");
            query_builder.push_bind(name);
        }

        query_builder.push(" WHERE id = ");
        query_builder.push_bind(self.id);
        query_builder.push(" RETURNING *");

        let segment = query_builder
            .build_query_as::<Segment>()
            .fetch_one(&mut *conn)
            .await
            .map_err(AppError::Database)?;

        Ok(segment)
    }
}

impl UpdateSegmentCommandBuilder {
    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn build(self) -> Result<UpdateSegmentCommand, AppError> {
        Ok(UpdateSegmentCommand {
            id: self
                .id
                .ok_or_else(|| AppError::Validation("id is required".into()))?,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            name: self.name,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct DeleteSegmentCommand {
    id: i64,
    deployment_id: i64,
}

#[derive(Default)]
pub struct DeleteSegmentCommandBuilder {
    id: Option<i64>,
    deployment_id: Option<i64>,
}

impl DeleteSegmentCommand {
    pub fn builder() -> DeleteSegmentCommandBuilder {
        DeleteSegmentCommandBuilder::default()
    }
}

impl DeleteSegmentCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<serde_json::Value, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = sqlx::query(
            "UPDATE segments SET deleted_at = NOW() WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.id)
        .bind(self.deployment_id)
        .execute(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Segment not found".into()));
        }

        // Clean up associations
        sqlx::query("DELETE FROM organization_segments WHERE segment_id = $1")
            .bind(self.id)
            .execute(&mut *conn)
            .await
            .map_err(AppError::Database)?;

        sqlx::query("DELETE FROM workspace_segments WHERE segment_id = $1")
            .bind(self.id)
            .execute(&mut *conn)
            .await
            .map_err(AppError::Database)?;

        sqlx::query("DELETE FROM user_segments WHERE segment_id = $1")
            .bind(self.id)
            .execute(&mut *conn)
            .await
            .map_err(AppError::Database)?;

        Ok(serde_json::json!({ "success": true }))
    }
}

impl DeleteSegmentCommandBuilder {
    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn build(self) -> Result<DeleteSegmentCommand, AppError> {
        Ok(DeleteSegmentCommand {
            id: self
                .id
                .ok_or_else(|| AppError::Validation("id is required".into()))?,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct AssignSegmentCommand {
    segment_id: i64,
    deployment_id: i64,
    entity_id: i64,
}

#[derive(Default)]
pub struct AssignSegmentCommandBuilder {
    segment_id: Option<i64>,
    deployment_id: Option<i64>,
    entity_id: Option<i64>,
}

impl AssignSegmentCommand {
    pub fn builder() -> AssignSegmentCommandBuilder {
        AssignSegmentCommandBuilder::default()
    }
}

impl AssignSegmentCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<serde_json::Value, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let segment_row = sqlx::query(
            "SELECT type FROM segments WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.segment_id)
        .bind(self.deployment_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        let segment_type: String = match segment_row {
            Some(row) => row.try_get("type").unwrap_or_default(),
            None => return Err(AppError::NotFound("Segment not found".into())),
        };

        match segment_type.as_str() {
            "organization" => {
                let org_exists = sqlx::query(
                    "SELECT id FROM organizations WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
                )
                .bind(self.entity_id)
                .bind(self.deployment_id)
                .fetch_optional(&mut *conn)
                .await
                .map_err(AppError::Database)?;

                if org_exists.is_none() {
                    return Err(AppError::NotFound("Organization not found".into()));
                }

                sqlx::query(
                    "INSERT INTO organization_segments (organization_id, segment_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(self.entity_id)
                .bind(self.segment_id)
                .execute(&mut *conn)
                .await
                .map_err(AppError::Database)?;
            }
            "workspace" => {
                let ws_exists = sqlx::query(
                    "SELECT id FROM workspaces WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
                )
                .bind(self.entity_id)
                .bind(self.deployment_id)
                .fetch_optional(&mut *conn)
                .await
                .map_err(AppError::Database)?;

                if ws_exists.is_none() {
                    return Err(AppError::NotFound("Workspace not found".into()));
                }

                sqlx::query(
                    "INSERT INTO workspace_segments (workspace_id, segment_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(self.entity_id)
                .bind(self.segment_id)
                .execute(&mut *conn)
                .await
                .map_err(AppError::Database)?;
            }
            "user" => {
                let user_exists = sqlx::query(
                    "SELECT id FROM users WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
                )
                .bind(self.entity_id)
                .bind(self.deployment_id)
                .fetch_optional(&mut *conn)
                .await
                .map_err(AppError::Database)?;

                if user_exists.is_none() {
                    return Err(AppError::NotFound("User not found".into()));
                }

                sqlx::query(
                    "INSERT INTO user_segments (user_id, segment_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
                )
                .bind(self.entity_id)
                .bind(self.segment_id)
                .execute(&mut *conn)
                .await
                .map_err(AppError::Database)?;
            }
            _ => return Err(AppError::Internal("Invalid segment type".into())),
        }

        Ok(serde_json::json!({ "success": true }))
    }
}

impl AssignSegmentCommandBuilder {
    pub fn segment_id(mut self, segment_id: i64) -> Self {
        self.segment_id = Some(segment_id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn entity_id(mut self, entity_id: i64) -> Self {
        self.entity_id = Some(entity_id);
        self
    }

    pub fn build(self) -> Result<AssignSegmentCommand, AppError> {
        Ok(AssignSegmentCommand {
            segment_id: self
                .segment_id
                .ok_or_else(|| AppError::Validation("segment_id is required".into()))?,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            entity_id: self
                .entity_id
                .ok_or_else(|| AppError::Validation("entity_id is required".into()))?,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct RemoveSegmentCommand {
    segment_id: i64,
    deployment_id: i64,
    entity_id: i64,
}

#[derive(Default)]
pub struct RemoveSegmentCommandBuilder {
    segment_id: Option<i64>,
    deployment_id: Option<i64>,
    entity_id: Option<i64>,
}

impl RemoveSegmentCommand {
    pub fn builder() -> RemoveSegmentCommandBuilder {
        RemoveSegmentCommandBuilder::default()
    }
}

impl RemoveSegmentCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<serde_json::Value, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let segment_row = sqlx::query(
            "SELECT type FROM segments WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.segment_id)
        .bind(self.deployment_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(AppError::Database)?;

        let segment_type: String = match segment_row {
            Some(row) => row.try_get("type").unwrap_or_default(),
            None => return Err(AppError::NotFound("Segment not found".into())),
        };

        match segment_type.as_str() {
            "organization" => {
                sqlx::query(
                    "DELETE FROM organization_segments WHERE organization_id = $1 AND segment_id = $2",
                )
                .bind(self.entity_id)
                .bind(self.segment_id)
                .execute(&mut *conn)
                .await
                .map_err(AppError::Database)?;
            }
            "workspace" => {
                sqlx::query(
                    "DELETE FROM workspace_segments WHERE workspace_id = $1 AND segment_id = $2",
                )
                .bind(self.entity_id)
                .bind(self.segment_id)
                .execute(&mut *conn)
                .await
                .map_err(AppError::Database)?;
            }
            "user" => {
                sqlx::query("DELETE FROM user_segments WHERE user_id = $1 AND segment_id = $2")
                    .bind(self.entity_id)
                    .bind(self.segment_id)
                    .execute(&mut *conn)
                    .await
                    .map_err(AppError::Database)?;
            }
            _ => return Err(AppError::Internal("Invalid segment type".into())),
        }

        Ok(serde_json::json!({ "success": true }))
    }
}

impl RemoveSegmentCommandBuilder {
    pub fn segment_id(mut self, segment_id: i64) -> Self {
        self.segment_id = Some(segment_id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn entity_id(mut self, entity_id: i64) -> Self {
        self.entity_id = Some(entity_id);
        self
    }

    pub fn build(self) -> Result<RemoveSegmentCommand, AppError> {
        Ok(RemoveSegmentCommand {
            segment_id: self
                .segment_id
                .ok_or_else(|| AppError::Validation("segment_id is required".into()))?,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            entity_id: self
                .entity_id
                .ok_or_else(|| AppError::Validation("entity_id is required".into()))?,
        })
    }
}
