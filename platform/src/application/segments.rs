use commands::{
    Command,
    segments::{
        AssignSegmentCommand, CreateSegmentCommand, DeleteSegmentCommand, RemoveSegmentCommand,
        UpdateSegmentCommand,
    },
};
use models::{AnalyzedEntity, Segment};
use queries::{
    Query as QueryTrait,
    segments::{GetSegmentDataQuery, GetSegmentsQuery},
};

use crate::application::{AppError, AppState};

pub fn validate_segment_type(segment_type: &str) -> Result<(), AppError> {
    if matches!(segment_type, "organization" | "workspace" | "user") {
        Ok(())
    } else {
        Err(AppError::BadRequest(
            "Invalid segment type. Must be 'organization', 'workspace', or 'user'".into(),
        ))
    }
}

pub async fn get_segment_data(
    app_state: &AppState,
    query: GetSegmentDataQuery,
) -> Result<Vec<AnalyzedEntity>, AppError> {
    query.execute(app_state).await
}

pub async fn list_segments(
    app_state: &AppState,
    query: GetSegmentsQuery,
) -> Result<Vec<Segment>, AppError> {
    query.execute(app_state).await
}

pub async fn create_segment(
    app_state: &AppState,
    deployment_id: i64,
    name: String,
    segment_type: String,
) -> Result<Segment, AppError> {
    CreateSegmentCommand {
        deployment_id,
        name,
        r#type: segment_type,
    }
    .execute(app_state)
    .await
}

pub async fn update_segment(
    app_state: &AppState,
    id: i64,
    deployment_id: i64,
    name: Option<String>,
) -> Result<Segment, AppError> {
    UpdateSegmentCommand {
        id,
        deployment_id,
        name,
    }
    .execute(app_state)
    .await
}

pub async fn delete_segment(
    app_state: &AppState,
    id: i64,
    deployment_id: i64,
) -> Result<serde_json::Value, AppError> {
    DeleteSegmentCommand { id, deployment_id }.execute(app_state).await
}

pub async fn assign_segment(
    app_state: &AppState,
    segment_id: i64,
    deployment_id: i64,
    entity_id: i64,
) -> Result<serde_json::Value, AppError> {
    AssignSegmentCommand {
        segment_id,
        deployment_id,
        entity_id,
    }
    .execute(app_state)
    .await
}

pub async fn remove_segment(
    app_state: &AppState,
    segment_id: i64,
    deployment_id: i64,
    entity_id: i64,
) -> Result<serde_json::Value, AppError> {
    RemoveSegmentCommand {
        segment_id,
        deployment_id,
        entity_id,
    }
    .execute(app_state)
    .await
}
