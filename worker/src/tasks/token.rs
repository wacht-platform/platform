use anyhow::Result;
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
) -> Result<String, String> {
    info!(
        "Token cleanup: rotating_token_id={}, session_id={}",
        rotating_token_id, session_id
    );

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    if let Err(e) = cleanup_rotating_token(rotating_token_id).await {
        error!(
            "Failed to cleanup rotating token {}: {}",
            rotating_token_id, e
        );
        return Err(format!("Token cleanup failed: {}", e));
    }

    if let Err(e) = cleanup_session(session_id).await {
        error!("Failed to cleanup session {}: {}", session_id, e);
        return Err(format!("Session cleanup failed: {}", e));
    }

    Ok(format!(
        "Token cleanup completed: {}/{}",
        rotating_token_id, session_id
    ))
}

async fn cleanup_rotating_token(rotating_token_id: u64) -> Result<(), String> {
    info!("Cleaned up rotating token: {}", rotating_token_id);
    Ok(())
}

async fn cleanup_session(session_id: u64) -> Result<(), String> {
    info!("Cleaned up session: {}", session_id);
    Ok(())
}
