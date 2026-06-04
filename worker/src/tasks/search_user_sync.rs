use crate::consumer::TaskError;
use common::state::AppState;
use dto::json::nats::SearchUserSyncPayload;
use queries::SyncSearchUserQuery;

/// Rebuilds one user's `search_users` row. Idempotent: re-running for the same
/// user just refreshes the row, so duplicate/at-least-once delivery is safe.
pub async fn sync_user(
    payload: SearchUserSyncPayload,
    app_state: &AppState,
) -> Result<String, TaskError> {
    SyncSearchUserQuery::new(payload.user_id)
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| {
            TaskError::Permanent(format!(
                "Failed to sync search user {}: {}",
                payload.user_id, e
            ))
        })?;

    Ok(format!("Synced search user {}", payload.user_id))
}
