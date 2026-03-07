use common::error::AppError;
use models::Segment;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct CreateSegmentCommand {
    id: Option<i64>,
    deployment_id: i64,
    name: String,
    r#type: String,
}

#[derive(Default)]
pub struct CreateSegmentCommandBuilder {
    id: Option<i64>,
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
    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Segment, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let id = self
            .id
            .ok_or_else(|| AppError::Validation("id is required".into()))?;
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
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(segment)
    }
}

impl CreateSegmentCommandBuilder {
    pub fn id(mut self, id: i64) -> Self {
        self.id = Some(id);
        self
    }

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
            id: self.id,
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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Segment, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let segment = sqlx::query_as!(
            Segment,
            r#"
            UPDATE segments
            SET
                updated_at = NOW(),
                name = COALESCE($3, name)
            WHERE id = $1
              AND deployment_id = $2
              AND deleted_at IS NULL
            RETURNING
                id,
                created_at,
                updated_at,
                deleted_at,
                deployment_id,
                name,
                type as "segment_type!"
            "#,
            self.id,
            self.deployment_id,
            self.name
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)?
        .ok_or_else(|| AppError::NotFound("Segment not found".into()))?;

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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<serde_json::Value, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH updated_segment AS (
                UPDATE segments
                SET deleted_at = NOW()
                WHERE id = $1
                  AND deployment_id = $2
                  AND deleted_at IS NULL
                RETURNING id
            ),
            deleted_org AS (
                DELETE FROM organization_segments
                WHERE segment_id = $1
                  AND EXISTS(SELECT 1 FROM updated_segment)
            ),
            deleted_ws AS (
                DELETE FROM workspace_segments
                WHERE segment_id = $1
                  AND EXISTS(SELECT 1 FROM updated_segment)
            ),
            deleted_user AS (
                DELETE FROM user_segments
                WHERE segment_id = $1
                  AND EXISTS(SELECT 1 FROM updated_segment)
            )
            SELECT EXISTS(SELECT 1 FROM updated_segment) AS "segment_exists!"
            "#,
            self.id,
            self.deployment_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        if !result.segment_exists {
            return Err(AppError::NotFound("Segment not found".into()));
        }

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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<serde_json::Value, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH segment AS (
                SELECT type
                FROM segments
                WHERE id = $1
                  AND deployment_id = $2
                  AND deleted_at IS NULL
            ),
            organization_exists AS (
                SELECT EXISTS(
                    SELECT 1
                    FROM organizations
                    WHERE id = $3
                      AND deployment_id = $2
                      AND deleted_at IS NULL
                ) AS v
            ),
            workspace_exists AS (
                SELECT EXISTS(
                    SELECT 1
                    FROM workspaces
                    WHERE id = $3
                      AND deployment_id = $2
                      AND deleted_at IS NULL
                ) AS v
            ),
            user_exists AS (
                SELECT EXISTS(
                    SELECT 1
                    FROM users
                    WHERE id = $3
                      AND deployment_id = $2
                      AND deleted_at IS NULL
                ) AS v
            ),
            ins_org AS (
                INSERT INTO organization_segments (organization_id, segment_id)
                SELECT $3, $1
                WHERE EXISTS(SELECT 1 FROM segment WHERE type = 'organization')
                  AND (SELECT v FROM organization_exists)
                ON CONFLICT DO NOTHING
            ),
            ins_ws AS (
                INSERT INTO workspace_segments (workspace_id, segment_id)
                SELECT $3, $1
                WHERE EXISTS(SELECT 1 FROM segment WHERE type = 'workspace')
                  AND (SELECT v FROM workspace_exists)
                ON CONFLICT DO NOTHING
            ),
            ins_user AS (
                INSERT INTO user_segments (user_id, segment_id)
                SELECT $3, $1
                WHERE EXISTS(SELECT 1 FROM segment WHERE type = 'user')
                  AND (SELECT v FROM user_exists)
                ON CONFLICT DO NOTHING
            )
            SELECT
                (SELECT type FROM segment LIMIT 1) AS "segment_type?",
                (SELECT v FROM organization_exists) AS "organization_exists!",
                (SELECT v FROM workspace_exists) AS "workspace_exists!",
                (SELECT v FROM user_exists) AS "user_exists!"
            "#,
            self.segment_id,
            self.deployment_id,
            self.entity_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        let Some(segment_type) = result.segment_type else {
            return Err(AppError::NotFound("Segment not found".into()));
        };

        match segment_type.as_str() {
            "organization" => {
                if !result.organization_exists {
                    return Err(AppError::NotFound("Organization not found".into()));
                }
            }
            "workspace" => {
                if !result.workspace_exists {
                    return Err(AppError::NotFound("Workspace not found".into()));
                }
            }
            "user" => {
                if !result.user_exists {
                    return Err(AppError::NotFound("User not found".into()));
                }
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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<serde_json::Value, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH segment AS (
                SELECT type
                FROM segments
                WHERE id = $1
                  AND deployment_id = $2
                  AND deleted_at IS NULL
            ),
            deleted_org AS (
                DELETE FROM organization_segments
                WHERE organization_id = $3
                  AND segment_id = $1
                  AND EXISTS(SELECT 1 FROM segment WHERE type = 'organization')
            ),
            deleted_ws AS (
                DELETE FROM workspace_segments
                WHERE workspace_id = $3
                  AND segment_id = $1
                  AND EXISTS(SELECT 1 FROM segment WHERE type = 'workspace')
            ),
            deleted_user AS (
                DELETE FROM user_segments
                WHERE user_id = $3
                  AND segment_id = $1
                  AND EXISTS(SELECT 1 FROM segment WHERE type = 'user')
            )
            SELECT
                (SELECT type FROM segment LIMIT 1) AS "segment_type?"
            "#,
            self.segment_id,
            self.deployment_id,
            self.entity_id
        )
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        let Some(segment_type) = result.segment_type else {
            return Err(AppError::NotFound("Segment not found".into()));
        };
        if segment_type != "organization" && segment_type != "workspace" && segment_type != "user" {
            return Err(AppError::Internal("Invalid segment type".into()));
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
