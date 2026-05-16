// Distributed synchronization for rate limiting
// Handles NATS deltas and Redis snapshots

use base64::Engine;
use chrono::Utc;
use common::state::AppState;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::window::BucketedWindow;

const REDIS_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const REDIS_OP_TIMEOUT: Duration = Duration::from_millis(500);

/// Request for a snapshot from another gateway
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub key: String,
    pub requesting_gateway: String,
}

/// Response containing a snapshot
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub gateway_id: String,
    pub timestamp_ms: i64,
    pub data: String, // Base64 encoded compressed data
}

/// Fetch a window snapshot from Redis. Bounded by `REDIS_CONNECT_TIMEOUT` and
/// `REDIS_OP_TIMEOUT`; on timeout or any error returns `None` so callers fall
/// back to a fresh window rather than hanging the request path.
pub async fn fetch_from_redis(app_state: &AppState, key: &str) -> Option<BucketedWindow> {
    let stream_key = format!("rate_limit:snapshot:{}", key);
    let mut conn = tokio::time::timeout(
        REDIS_CONNECT_TIMEOUT,
        app_state.redis_client.get_multiplexed_async_connection(),
    )
    .await
    .ok()?
    .ok()?;

    let result: Vec<(String, std::collections::HashMap<String, String>)> = tokio::time::timeout(
        REDIS_OP_TIMEOUT,
        redis::cmd("XREVRANGE")
            .arg(&stream_key)
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg("1")
            .query_async(&mut conn),
    )
    .await
    .ok()?
    .ok()?;

    result
        .first()?
        .1
        .get("data")
        .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .and_then(|compressed| BucketedWindow::from_compressed(&compressed).ok())
}

/// Publish a pre-compressed window snapshot to Redis. Caller is responsible
/// for producing `compressed` under any locks it needs; this function never
/// holds those locks across its awaits. Bounded by `REDIS_CONNECT_TIMEOUT` and
/// `REDIS_OP_TIMEOUT` so a slow/dead Redis cannot stall request-path logic.
pub async fn publish_to_redis(
    app_state: &AppState,
    gateway_id: &str,
    key: &str,
    compressed: Vec<u8>,
) {
    let compressed_base64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    let stream_key = format!("rate_limit:snapshot:{}", key);
    let timestamp_ms = Utc::now().timestamp_millis();

    let Ok(Ok(mut conn)) = tokio::time::timeout(
        REDIS_CONNECT_TIMEOUT,
        app_state.redis_client.get_multiplexed_async_connection(),
    )
    .await
    else {
        return;
    };

    let mut pipe = redis::pipe();
    pipe.cmd("XADD")
        .arg(&stream_key)
        .arg(timestamp_ms.to_string())
        .arg("gateway_id")
        .arg(gateway_id)
        .arg("timestamp_ms")
        .arg(timestamp_ms)
        .arg("data")
        .arg(&compressed_base64);
    pipe.cmd("XTRIM")
        .arg(&stream_key)
        .arg("MAXLEN")
        .arg("~")
        .arg(100);

    let _: Result<(), _> = tokio::time::timeout(REDIS_OP_TIMEOUT, pipe.query_async(&mut conn))
        .await
        .unwrap_or(Ok(()));
}
