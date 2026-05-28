use redis::AsyncCommands;
use serde::{Serialize, de::DeserializeOwned};

/// Read a JSON-encoded value from Redis. Returns `None` on any failure
/// (network, parse, missing key) — cache misses must fail open to the
/// caller's DB fallback.
pub async fn read_cache<T>(redis: &redis::Client, key: &str) -> Option<T>
where
    T: DeserializeOwned,
{
    let mut conn = redis.get_multiplexed_async_connection().await.ok()?;
    let json: String = conn.get(key).await.ok()?;
    serde_json::from_str(&json).ok()
}

/// JSON-encode `value` and write it to Redis with `SETEX`. Silent on
/// failure — caching is best-effort.
pub async fn write_cache<T>(redis: &redis::Client, key: &str, value: &T, ttl_seconds: u64)
where
    T: Serialize,
{
    let Ok(json) = serde_json::to_string(value) else {
        return;
    };
    let Ok(mut conn) = redis.get_multiplexed_async_connection().await else {
        return;
    };
    let _: Result<(), _> = conn.set_ex(key, json, ttl_seconds).await;
}

/// Delete one or more keys. Silent on failure.
pub async fn invalidate_cache(redis: &redis::Client, keys: &[String]) {
    if keys.is_empty() {
        return;
    }
    let Ok(mut conn) = redis.get_multiplexed_async_connection().await else {
        return;
    };
    let _: Result<(), _> = conn.del(keys).await;
}
