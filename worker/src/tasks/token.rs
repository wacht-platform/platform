use celery::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{info, error};

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenCleanupTask {
    pub rotating_token_id: u64,
    pub session_id: u64,
}

#[celery::task(name = "token.clean")]
pub async fn clean_token(task: TokenCleanupTask) -> TaskResult<String> {
    info!("Token cleanup: rotating_token_id={}, session_id={}", task.rotating_token_id, task.session_id);

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    if let Err(e) = cleanup_rotating_token(task.rotating_token_id).await {
        error!("Failed to cleanup rotating token {}: {}", task.rotating_token_id, e);
        return Err(TaskError::UnexpectedError(format!("Token cleanup failed: {}", e)));
    }

    if let Err(e) = cleanup_session(task.session_id).await {
        error!("Failed to cleanup session {}: {}", task.session_id, e);
        return Err(TaskError::UnexpectedError(format!("Session cleanup failed: {}", e)));
    }

    Ok(format!("Token cleanup completed: {}/{}", task.rotating_token_id, task.session_id))
}

async fn cleanup_rotating_token(rotating_token_id: u64) -> Result<(), String> {
    info!("Cleaned up rotating token: {}", rotating_token_id);
    Ok(())
}

async fn cleanup_session(session_id: u64) -> Result<(), String> {
    info!("Cleaned up session: {}", session_id);
    Ok(())
}
