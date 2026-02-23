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

    let inserted: bool = redis_conn
        .set_nx(key, chrono::Utc::now().to_rfc3339())
        .await
        .map_err(|e| AppError::Internal(format!("Redis set_nx failed: {}", e)))?;

    if inserted {
        let _: Result<bool, redis::RedisError> = redis_conn.expire(key, ttl_seconds as i64).await;
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
