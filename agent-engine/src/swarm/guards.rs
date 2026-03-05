use common::error::AppError;
use common::state::AppState;
use redis::AsyncCommands;
use std::hash::{Hash, Hasher};

pub async fn acquire_dedupe_token(
    app_state: &AppState,
    key: &str,
    ttl_seconds: u64,
) -> Result<bool, AppError> {
    let mut redis_conn = app_state
        .redis_client
        .get_multiplexed_tokio_connection()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to connect to Redis: {}", e)))?;

    let set_response: Option<String> = redis::cmd("SET")
        .arg(key)
        .arg(chrono::Utc::now().to_rfc3339())
        .arg("NX")
        .arg("EX")
        .arg(ttl_seconds)
        .query_async(&mut redis_conn)
        .await
        .map_err(|e| AppError::Internal(format!("Redis SET NX EX failed: {}", e)))?;

    if set_response.as_deref() == Some("OK") {
        Ok(false)
    } else {
        Ok(true)
    }
}

pub async fn clear_token(app_state: &AppState, key: &str) {
    if let Ok(mut redis_conn) = app_state
        .redis_client
        .get_multiplexed_tokio_connection()
        .await
    {
        let _: Result<(), redis::RedisError> = redis_conn.del(key).await;
    }
}

pub fn stable_fingerprint<T: Hash>(value: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
