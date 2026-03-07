use anyhow::Result;
use commands::{CleanupOrphanSessionCommand, CleanupRotatingTokenCommand};
use common::state::AppState;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenCleanupTask {
    pub rotating_token_id: u64,
    pub session_id: u64,
}

pub async fn cleanup_rotating_token_and_session(
    rotating_token_id: u64,
    session_id: u64,
    app_state: &AppState,
) -> Result<String, String> {
    info!(
        "Token cleanup: rotating_token_id={}, session_id={}",
        rotating_token_id, session_id
    );

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    if let Err(e) = cleanup_rotating_token(rotating_token_id, app_state).await {
        error!(
            "Failed to cleanup rotating token {}: {}",
            rotating_token_id, e
        );
        return Err(format!("Token cleanup failed: {}", e));
    }

    if let Err(e) = cleanup_session(session_id, app_state).await {
        error!("Failed to cleanup session {}: {}", session_id, e);
        return Err(format!("Session cleanup failed: {}", e));
    }

    Ok(format!(
        "Token cleanup completed: {}/{}",
        rotating_token_id, session_id
    ))
}

async fn cleanup_rotating_token(
    rotating_token_id: u64,
    app_state: &AppState,
) -> Result<(), String> {
    let rotating_token_id =
        i64::try_from(rotating_token_id).map_err(|_| "rotating_token_id overflow".to_string())?;

    let deleted = CleanupRotatingTokenCommand { rotating_token_id }
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| e.to_string())?;

    if deleted {
        info!("Cleaned up rotating token: {}", rotating_token_id);
    } else {
        info!(
            "Skipped rotating token cleanup (not eligible): {}",
            rotating_token_id
        );
    }

    Ok(())
}

async fn cleanup_session(session_id: u64, app_state: &AppState) -> Result<(), String> {
    let session_id = i64::try_from(session_id).map_err(|_| "session_id overflow".to_string())?;

    let deleted = CleanupOrphanSessionCommand { session_id }
        .execute_with_db(app_state.db_router.writer())
        .await
        .map_err(|e| e.to_string())?;

    if deleted {
        info!("Cleaned up orphaned session: {}", session_id);
    } else {
        info!("Skipped session cleanup (not orphaned): {}", session_id);
    }

    Ok(())
}
