use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::Segment;
use serde::{Deserialize, Serialize};
use sqlx::Row;

#[derive(Serialize, Deserialize)]
pub struct CreateSegmentCommand {
    pub deployment_id: i64,
    pub name: String,
    pub r#type: String,
}

impl Command for CreateSegmentCommand {
    type Output = Segment;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let id = app_state.sf.next_id()? as i64;
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
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        Ok(segment)
    }
}

#[derive(Serialize, Deserialize)]
pub struct UpdateSegmentCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub name: Option<String>,
}

impl Command for UpdateSegmentCommand {
    type Output = Segment;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let existing = sqlx::query(
            "SELECT id FROM segments WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.id)
        .bind(self.deployment_id)
        .fetch_optional(&app_state.db_pool)
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
            .fetch_one(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

        Ok(segment)
    }
}

#[derive(Serialize, Deserialize)]
pub struct DeleteSegmentCommand {
    pub id: i64,
    pub deployment_id: i64,
}

impl Command for DeleteSegmentCommand {
    type Output = serde_json::Value;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query(
            "UPDATE segments SET deleted_at = NOW() WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.id)
        .bind(self.deployment_id)
        .execute(&app_state.db_pool)
        .await
        .map_err(AppError::Database)?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Segment not found".into()));
        }

        // Clean up associations
        sqlx::query("DELETE FROM organization_segments WHERE segment_id = $1")
            .bind(self.id)
            .execute(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

        sqlx::query("DELETE FROM workspace_segments WHERE segment_id = $1")
            .bind(self.id)
            .execute(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

        sqlx::query("DELETE FROM user_segments WHERE segment_id = $1")
            .bind(self.id)
            .execute(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

        Ok(serde_json::json!({ "success": true }))
    }
}

#[derive(Serialize, Deserialize)]
pub struct AssignSegmentCommand {
    pub segment_id: i64,
    pub deployment_id: i64,
    pub entity_id: i64,
}

impl Command for AssignSegmentCommand {
    type Output = serde_json::Value;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let segment_row = sqlx::query(
            "SELECT type FROM segments WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.segment_id)
        .bind(self.deployment_id)
        .fetch_optional(&app_state.db_pool)
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
                .fetch_optional(&app_state.db_pool)
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
                .execute(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?;
            }
            "workspace" => {
                let ws_exists = sqlx::query(
                    "SELECT id FROM workspaces WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
                )
                .bind(self.entity_id)
                .bind(self.deployment_id)
                .fetch_optional(&app_state.db_pool)
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
                .execute(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?;
            }
            "user" => {
                let user_exists = sqlx::query(
                    "SELECT id FROM users WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
                )
                .bind(self.entity_id)
                .bind(self.deployment_id)
                .fetch_optional(&app_state.db_pool)
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
                .execute(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?;
            }
            _ => return Err(AppError::Internal("Invalid segment type".into())),
        }

        Ok(serde_json::json!({ "success": true }))
    }
}

#[derive(Serialize, Deserialize)]
pub struct RemoveSegmentCommand {
    pub segment_id: i64,
    pub deployment_id: i64,
    pub entity_id: i64,
}

impl Command for RemoveSegmentCommand {
    type Output = serde_json::Value;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let segment_row = sqlx::query(
            "SELECT type FROM segments WHERE id = $1 AND deployment_id = $2 AND deleted_at IS NULL",
        )
        .bind(self.segment_id)
        .bind(self.deployment_id)
        .fetch_optional(&app_state.db_pool)
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
                .execute(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?;
            }
            "workspace" => {
                sqlx::query(
                    "DELETE FROM workspace_segments WHERE workspace_id = $1 AND segment_id = $2",
                )
                .bind(self.entity_id)
                .bind(self.segment_id)
                .execute(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?;
            }
            "user" => {
                sqlx::query("DELETE FROM user_segments WHERE user_id = $1 AND segment_id = $2")
                    .bind(self.entity_id)
                    .bind(self.segment_id)
                    .execute(&app_state.db_pool)
                    .await
                    .map_err(AppError::Database)?;
            }
            _ => return Err(AppError::Internal("Invalid segment type".into())),
        }

        Ok(serde_json::json!({ "success": true }))
    }
}
