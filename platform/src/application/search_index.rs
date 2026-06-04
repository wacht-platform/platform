use common::state::AppState;
use dto::json::NatsTaskMessage;
use dto::json::nats::SearchUserSyncPayload;
use tracing::error;

/// Best-effort enqueue of a search-index refresh for a user. The worker owns the
/// actual denormalization (SyncSearchUserQuery); here we only publish the id. A
/// failed publish must never fail the user-facing write — the periodic backfill
/// reconciles any stragglers.
pub async fn publish_search_user_sync(app_state: &AppState, user_id: i64) {
    let payload = match serde_json::to_value(SearchUserSyncPayload { user_id }) {
        Ok(v) => v,
        Err(e) => {
            error!(user_id, error = %e, "failed to encode search.sync_user payload");
            return;
        }
    };

    let message = NatsTaskMessage {
        task_type: "search.sync_user".to_string(),
        task_id: format!("search-user-{}", user_id),
        payload,
    };

    let bytes = match serde_json::to_vec(&message) {
        Ok(b) => b,
        Err(e) => {
            error!(user_id, error = %e, "failed to encode search.sync_user message");
            return;
        }
    };

    if let Err(e) = app_state
        .nats_client
        .publish("worker.tasks.search.sync_user", bytes.into())
        .await
    {
        error!(user_id, error = %e, "failed to publish search.sync_user");
    }
}
