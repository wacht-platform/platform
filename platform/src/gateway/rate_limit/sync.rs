// Distributed synchronization for rate limiting
// Handles NATS deltas and Redis snapshots

use base64::Engine;
use chrono::Utc;
use common::state::AppState;
use serde::{Deserialize, Serialize};

use super::window::BucketedWindow;

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

/// Fetch a window snapshot from Redis
pub async fn fetch_from_redis(app_state: &AppState, key: &str) -> Option<BucketedWindow> {
    let stream_key = format!("rate_limit:snapshot:{}", key);
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .ok()?;

    let result: Vec<(String, std::collections::HashMap<String, String>)> = redis::cmd("XREVRANGE")
        .arg(&stream_key)
        .arg("+")
        .arg("-")
        .arg("COUNT")
        .arg("1")
        .query_async(&mut conn)
        .await
        .ok()?;

    result
        .first()?
        .1
        .get("data")
        .and_then(|data| base64::engine::general_purpose::STANDARD.decode(data).ok())
        .and_then(|compressed| BucketedWindow::from_compressed(&compressed).ok())
}

/// Publish a window snapshot to Redis
pub async fn publish_to_redis(
    app_state: &AppState,
    gateway_id: &str,
    key: &str,
    window: &BucketedWindow,
) {
    let compressed = match window.to_compressed() {
        Ok(c) => c,
        Err(_) => return,
    };

    let compressed_base64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    let stream_key = format!("rate_limit:snapshot:{}", key);
    let timestamp_ms = Utc::now().timestamp_millis();

    let Ok(mut conn) = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await
    else {
        return;
    };

    // Use pipeline to send XADD and XTRIM in one round trip
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

    let _: () = pipe.query_async(&mut conn).await.unwrap_or_default();
}
