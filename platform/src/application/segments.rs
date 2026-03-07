use commands::segments::{
    AssignSegmentCommand, CreateSegmentCommand, DeleteSegmentCommand, RemoveSegmentCommand,
    UpdateSegmentCommand,
};
use models::{AnalyzedEntity, Segment};
use queries::segments::{GetSegmentDataQuery, GetSegmentsQuery};

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
    query.execute_with_db(app_state.db_router.writer()).await
}

pub async fn list_segments(
    app_state: &AppState,
    query: GetSegmentsQuery,
) -> Result<Vec<Segment>, AppError> {
    query.execute_with_db(app_state.db_router.writer()).await
}

pub async fn create_segment(
    app_state: &AppState,
    deployment_id: i64,
    name: String,
    segment_type: String,
) -> Result<Segment, AppError> {
    let segment_id = app_state.sf.next_id()? as i64;
    CreateSegmentCommand::builder()
        .id(segment_id)
        .deployment_id(deployment_id)
        .name(name)
        .segment_type(segment_type)
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn update_segment(
    app_state: &AppState,
    id: i64,
    deployment_id: i64,
    name: Option<String>,
) -> Result<Segment, AppError> {
    UpdateSegmentCommand::builder()
        .id(id)
        .deployment_id(deployment_id)
        .name(name)
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn delete_segment(
    app_state: &AppState,
    id: i64,
    deployment_id: i64,
) -> Result<serde_json::Value, AppError> {
    DeleteSegmentCommand::builder()
        .id(id)
        .deployment_id(deployment_id)
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn assign_segment(
    app_state: &AppState,
    segment_id: i64,
    deployment_id: i64,
    entity_id: i64,
) -> Result<serde_json::Value, AppError> {
    AssignSegmentCommand::builder()
        .segment_id(segment_id)
        .deployment_id(deployment_id)
        .entity_id(entity_id)
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}

pub async fn remove_segment(
    app_state: &AppState,
    segment_id: i64,
    deployment_id: i64,
    entity_id: i64,
) -> Result<serde_json::Value, AppError> {
    RemoveSegmentCommand::builder()
        .segment_id(segment_id)
        .deployment_id(deployment_id)
        .entity_id(entity_id)
        .build()?
        .execute_with_db(app_state.db_router.writer())
        .await
}
