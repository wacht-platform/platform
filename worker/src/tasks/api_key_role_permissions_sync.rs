use crate::consumer::TaskError;
use common::db_router::ReadConsistency;
use common::state::AppState;
use dto::json::nats::{
    ApiKeyOrgMembershipSyncPayload, ApiKeyOrgRoleSyncPayload, ApiKeyWorkspaceMembershipSyncPayload,
    ApiKeyWorkspaceRoleSyncPayload,
};
use queries::api_key::{
    GetOrganizationMembershipIdsByRoleQuery, GetWorkspaceMembershipIdsByRoleQuery,
    SyncApiKeyOrgRolePermissionsForMembershipsQuery,
    SyncApiKeyWorkspaceRolePermissionsForMembershipsQuery,
};
use redis::AsyncCommands;
use tracing::info;

const BATCH_SIZE: usize = 500;
const CHECKPOINT_TTL_SECS: u64 = 6 * 60 * 60;

fn checkpoint_key(prefix: &str, id: i64) -> String {
    format!("api_key_perm_sync:{}:{}", prefix, id)
}

async fn load_checkpoint(app_state: &AppState, key: &str) -> Result<Option<i64>, TaskError> {
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| TaskError::Permanent(format!("Redis connection failed: {}", e)))?;
    let val: Option<i64> = conn
        .get(key)
        .await
        .map_err(|e| TaskError::Permanent(format!("Redis get failed: {}", e)))?;
    Ok(val)
}

async fn save_checkpoint(app_state: &AppState, key: &str, last_id: i64) -> Result<(), TaskError> {
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| TaskError::Permanent(format!("Redis connection failed: {}", e)))?;
    let _: () = conn
        .set_ex(key, last_id, CHECKPOINT_TTL_SECS)
        .await
        .map_err(|e| TaskError::Permanent(format!("Redis set failed: {}", e)))?;
    Ok(())
}

async fn clear_checkpoint(app_state: &AppState, key: &str) -> Result<(), TaskError> {
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|e| TaskError::Permanent(format!("Redis connection failed: {}", e)))?;
    let _: () = conn
        .del(key)
        .await
        .map_err(|e| TaskError::Permanent(format!("Redis del failed: {}", e)))?;
    Ok(())
}

fn chunk_ids(ids: Vec<i64>) -> Vec<Vec<i64>> {
    if ids.is_empty() {
        return vec![];
    }
    ids.chunks(BATCH_SIZE).map(|c| c.to_vec()).collect()
}

pub async fn sync_org_membership(
    payload: ApiKeyOrgMembershipSyncPayload,
    app_state: &AppState,
) -> Result<String, TaskError> {
    let writer = app_state.db_router.writer();
    let updated = SyncApiKeyOrgRolePermissionsForMembershipsQuery::new(vec![payload.membership_id])
        .execute_with(writer)
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to sync org membership: {}", e)))?;

    Ok(format!("Updated {} keys", updated.len()))
}

pub async fn sync_workspace_membership(
    payload: ApiKeyWorkspaceMembershipSyncPayload,
    app_state: &AppState,
) -> Result<String, TaskError> {
    let writer = app_state.db_router.writer();
    let updated =
        SyncApiKeyWorkspaceRolePermissionsForMembershipsQuery::new(vec![payload.membership_id])
            .execute_with(writer)
            .await
            .map_err(|e| {
                TaskError::Permanent(format!("Failed to sync workspace membership: {}", e))
            })?;

    Ok(format!("Updated {} keys", updated.len()))
}

pub async fn sync_org_role(
    payload: ApiKeyOrgRoleSyncPayload,
    app_state: &AppState,
) -> Result<String, TaskError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let mut membership_ids = GetOrganizationMembershipIdsByRoleQuery::new(payload.role_id)
        .execute_with(reader)
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to load org memberships: {}", e)))?;

    membership_ids.sort_unstable();

    let checkpoint = checkpoint_key("org_role", payload.role_id);
    if let Some(last_id) = load_checkpoint(app_state, &checkpoint).await? {
        membership_ids.retain(|id| *id > last_id);
    }

    let mut total_updated = 0usize;
    for batch in chunk_ids(membership_ids) {
        let writer = app_state.db_router.writer();
        let updated = SyncApiKeyOrgRolePermissionsForMembershipsQuery::new(batch)
            .execute_with(writer)
            .await
            .map_err(|e| TaskError::Permanent(format!("Failed to sync org role: {}", e)))?;
        total_updated += updated.len();

        if let Some(max_id) = updated.iter().copied().max() {
            save_checkpoint(app_state, &checkpoint, max_id).await?;
        }
    }

    clear_checkpoint(app_state, &checkpoint).await?;

    info!(
        "Synced org role {} -> {} keys",
        payload.role_id, total_updated
    );
    Ok(format!("Updated {} keys", total_updated))
}

pub async fn sync_workspace_role(
    payload: ApiKeyWorkspaceRoleSyncPayload,
    app_state: &AppState,
) -> Result<String, TaskError> {
    let reader = app_state.db_router.reader(ReadConsistency::Strong);
    let mut membership_ids = GetWorkspaceMembershipIdsByRoleQuery::new(payload.role_id)
        .execute_with(reader)
        .await
        .map_err(|e| {
            TaskError::Permanent(format!("Failed to load workspace memberships: {}", e))
        })?;

    membership_ids.sort_unstable();

    let checkpoint = checkpoint_key("workspace_role", payload.role_id);
    if let Some(last_id) = load_checkpoint(app_state, &checkpoint).await? {
        membership_ids.retain(|id| *id > last_id);
    }

    let mut total_updated = 0usize;
    for batch in chunk_ids(membership_ids) {
        let writer = app_state.db_router.writer();
        let updated = SyncApiKeyWorkspaceRolePermissionsForMembershipsQuery::new(batch)
            .execute_with(writer)
            .await
            .map_err(|e| TaskError::Permanent(format!("Failed to sync workspace role: {}", e)))?;
        total_updated += updated.len();

        if let Some(max_id) = updated.iter().copied().max() {
            save_checkpoint(app_state, &checkpoint, max_id).await?;
        }
    }

    clear_checkpoint(app_state, &checkpoint).await?;

    info!(
        "Synced workspace role {} -> {} keys",
        payload.role_id, total_updated
    );
    Ok(format!("Updated {} keys", total_updated))
}
