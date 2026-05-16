// RateLimiter service - main orchestrator for rate limiting
// Handles caching, window management, and distributed synchronization

use async_nats::jetstream::{self, stream::Config};
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine;
use chrono::Utc;
use common::state::AppState;
use dashmap::DashMap;
use futures::StreamExt;
use moka::future::Cache;
use queries::api_key_gateway::{ApiKeyGatewayData, GetApiKeyGatewayDataQuery};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::{debug, error, info};

use super::{
    cache::{CacheLookupError, CachedApiKeyData, CachedRateLimitSchemeData},
    sync::{self, SnapshotRequest, SnapshotResponse},
    window::BucketedWindow,
};
use crate::gateway::delta_stream::{DeltaPublisher, RateLimitDelta};

/// Rate limiter service with caching and distributed sync
#[derive(Clone)]
pub struct RateLimiter {
    /// Active rate limit windows keyed by limit identifier
    pub windows: Arc<DashMap<String, Arc<RwLock<BucketedWindow>>>>,
    /// Cache for API key data
    pub api_key_cache: Arc<Cache<String, Arc<CachedApiKeyData>>>,
    /// Cache for rate limit schemes
    pub rate_limit_scheme_cache: Arc<Cache<String, Arc<CachedRateLimitSchemeData>>>,
    /// Publisher for rate limit deltas
    pub delta_publisher: DeltaPublisher,

    app_state: AppState,
    nats_client: async_nats::Client,
    gateway_id: String,
}

impl RateLimiter {
    /// Create a new rate limiter with NATS/Redis connections
    pub async fn new(
        app_state: AppState,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let nats_client = app_state.nats_client.clone();
        let jetstream = jetstream::new(nats_client.clone());

        if jetstream.get_stream("rate_limit_deltas").await.is_err() {
            jetstream
                .create_stream(Config {
                    name: "rate_limit_deltas".to_string(),
                    subjects: vec!["rate_limit_deltas".to_string()],
                    max_age: Duration::from_secs(60),
                    max_messages: 10000,
                    ..Default::default()
                })
                .await?;
            info!("[RATE_LIMITER] Created jetstream for deltas");
        }

        info!("[RATE_LIMITER] Initialized NATS connection");

        let api_key_cache = Cache::builder()
            .time_to_live(Duration::from_secs(60))
            .max_capacity(100_000)
            .build();

        let rate_limit_scheme_cache = Cache::builder()
            .time_to_live(Duration::from_secs(3600))
            .max_capacity(10_000)
            .build();

        let gateway_id = format!(
            "gateway_{}_{}",
            std::env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string()),
            std::process::id()
        );
        let delta_publisher = DeltaPublisher::new();

        let limiter = Self {
            windows: Arc::new(DashMap::new()),
            api_key_cache: Arc::new(api_key_cache),
            rate_limit_scheme_cache: Arc::new(rate_limit_scheme_cache),
            delta_publisher,
            app_state,
            nats_client,
            gateway_id,
        };

        limiter.start_delta_consumer().await?;
        limiter.start_delta_publisher().await;
        limiter.start_snapshot_handler().await?;

        Ok(limiter)
    }

    /// Get or load a rate limit window by key.
    ///
    /// CRITICAL: DashMap uses synchronous parking_lot locks internally. Holding
    /// a `DashMap::entry()` guard across an `.await` deadlocks Tokio: a worker
    /// thread suspended while holding the shard lock can be picked to run
    /// another task that calls `entry()` on the same shard, which blocks the
    /// worker, leaving no thread free to resume the original future. On
    /// single-vCPU Cloud Run with a single Tokio worker thread, this hangs
    /// permanently the first time two requests race on a cold key.
    ///
    /// The fetch happens BEFORE we take any DashMap lock. Under a cold-cache
    /// race two requests may both hit Redis for the same key; one wins the
    /// insert, the other discards its fetched window and uses the existing
    /// arc. Wasted work is rare and bounded; deadlock is impossible.
    async fn get_or_load_window(&self, key: &str) -> Arc<RwLock<BucketedWindow>> {
        // Fast path: window already exists. Read guard from DashMap is dropped
        // at the end of this `if let`; we never hold it across the .await
        // below because the early return takes the only path that uses it.
        if let Some(window) = self.windows.get(key) {
            return window.clone();
        }

        // Slow path: fetch outside any DashMap lock.
        debug!("[RATE_LIMITER] Loading window: {}", key);
        let fetched = sync::fetch_from_redis(&self.app_state, key)
            .await
            .unwrap_or_else(|| {
                debug!("[RATE_LIMITER] Fresh window: {}", key);
                BucketedWindow::new()
            });
        let new_arc = Arc::new(RwLock::new(fetched));

        // Synchronous insert-or-get. No await is held across the entry guard.
        let key_owned = key.to_string();
        match self.windows.entry(key_owned) {
            dashmap::mapref::entry::Entry::Occupied(entry) => entry.get().clone(),
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                entry.insert(new_arc.clone());
                new_arc
            }
        }
    }

    /// Get API key data with request coalescing
    pub async fn get_api_key_data(
        &self,
        key_hash: String,
        app_state: &AppState,
    ) -> Result<ApiKeyGatewayData, Response> {
        let app_state_clone = app_state.clone();
        let key_hash_clone = key_hash.clone();

        let result = self
            .api_key_cache
            .try_get_with(key_hash, async move {
                match GetApiKeyGatewayDataQuery::new(key_hash_clone)
                    .execute_with_db(app_state_clone.db_router.writer())
                    .await
                {
                    Ok(Some(data)) => Ok(Arc::new(CachedApiKeyData {
                        data,
                        cached_at: Utc::now(),
                    })),
                    Ok(None) => Err(CacheLookupError::NotFound),
                    Err(_) => Err(CacheLookupError::DatabaseError),
                }
            })
            .await;

        match result {
            Ok(cached) => Ok(cached.data.clone()),
            Err(e) => {
                if matches!(*e, CacheLookupError::NotFound) {
                    Err((StatusCode::UNAUTHORIZED, "Invalid API key").into_response())
                } else {
                    Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response())
                }
            }
        }
    }

    /// Get rate limit scheme with request coalescing
    pub async fn get_rate_limit_scheme(
        &self,
        deployment_id: i64,
        slug: String,
        app_state: &AppState,
    ) -> Option<Vec<models::api_key::RateLimit>> {
        use queries::rate_limit_scheme::GetRateLimitSchemeQuery;

        let cache_key = format!("{}:{}", deployment_id, slug);
        let app_state_clone = app_state.clone();
        let slug_clone = slug.clone();

        let result = self
            .rate_limit_scheme_cache
            .try_get_with(cache_key, async move {
                match GetRateLimitSchemeQuery::new(deployment_id, slug_clone)
                    .execute_with_db(app_state_clone.db_router.writer())
                    .await
                {
                    Ok(Some(scheme)) => Ok(Arc::new(CachedRateLimitSchemeData {
                        data: scheme.rules,
                        cached_at: Utc::now(),
                    })),
                    _ => Err(CacheLookupError::NotFound),
                }
            })
            .await;

        result.ok().map(|cached| cached.data.clone())
    }

    /// Check rate limit and add request if allowed
    pub async fn check_rate_limit(
        &self,
        key: String,
        limit: u32,
        window_ms: i64,
        is_calendar_day: bool,
    ) -> (bool, u32, u32) {
        debug!(
            key = %key,
            limit = limit,
            window_ms = window_ms,
            "[RATE_LIMIT] Checking rate limit"
        );

        let window = self.get_or_load_window(&key).await;
        let (allowed, remaining, retry_after) = {
            let mut w = window.write().await;
            w.check_and_add_request(window_ms, limit, is_calendar_day)
        };

        if allowed {
            let delta = RateLimitDelta {
                key: key.clone(),
                gateway_id: self.gateway_id.clone(),
                delta: 1,
                timestamp: Utc::now().timestamp_millis(),
            };
            self.delta_publisher.publish(delta);

            // Publish snapshot to Redis asynchronously. Critical: we MUST NOT
            // hold the window's read lock across the Redis I/O — a stalled or
            // dead Redis would otherwise keep the read guard alive forever,
            // blocking every subsequent `window.write().await` on this key
            // and producing the classic "first few requests fine, then every
            // request 504s" deadlock. Compress under the lock, drop the lock,
            // then publish the owned bytes.
            let key_clone = key.clone();
            let window_clone = window.clone();
            let app_state = self.app_state.clone();
            let gateway_id = self.gateway_id.clone();
            tokio::spawn(async move {
                let compressed = {
                    let w = window_clone.read().await;
                    match w.to_compressed() {
                        Ok(c) => c,
                        Err(e) => {
                            error!(error = ?e, "[RATE_LIMIT] Snapshot compression failed");
                            return;
                        }
                    }
                };
                sync::publish_to_redis(&app_state, &gateway_id, &key_clone, compressed).await;
                debug!(key = %key_clone, "[RATE_LIMIT] Snapshot saved to Redis");
            });

            debug!(remaining = remaining, "[RATE_LIMIT] Allowed");
        } else {
            debug!(remaining = remaining, "[RATE_LIMIT] Blocked");
        }

        (allowed, remaining, retry_after)
    }

    // -------------------------------------------------------------------------
    // Background tasks
    // -------------------------------------------------------------------------

    async fn start_delta_consumer(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut subscriber = self.nats_client.subscribe("rate_limit_deltas").await?;
        let windows = self.windows.clone();
        let gateway_id = self.gateway_id.clone();

        tokio::spawn(async move {
            info!(gateway_id = %gateway_id, "[DELTA_CONSUMER] Started");

            while let Some(message) = subscriber.next().await {
                let Ok(delta) = serde_json::from_slice::<RateLimitDelta>(&message.payload) else {
                    continue;
                };

                if delta.gateway_id == gateway_id {
                    continue;
                }

                debug!(
                    key = %delta.key,
                    gateway = %delta.gateway_id,
                    "[DELTA_CONSUMER] Processing delta"
                );

                let window = windows
                    .entry(delta.key.clone())
                    .or_insert_with(|| Arc::new(RwLock::new(BucketedWindow::new())))
                    .clone();

                let mut w = window.write().await;
                w.apply_delta(delta.timestamp);
            }
        });

        Ok(())
    }

    async fn start_delta_publisher(&self) {
        let mut rx = self.delta_publisher.subscribe();
        let nats_client = self.nats_client.clone();

        tokio::spawn(async move {
            info!("[DELTA_PUBLISHER] Started");

            while let Ok(delta) = rx.recv().await {
                let delta_json = match serde_json::to_vec(&delta) {
                    Ok(j) => j,
                    Err(e) => {
                        error!("[DELTA_PUBLISHER] Error: {:?}", e);
                        continue;
                    }
                };

                if let Err(e) = nats_client
                    .publish("rate_limit_deltas", delta_json.into())
                    .await
                {
                    error!("[DELTA_PUBLISHER] Publish error: {:?}", e);
                }
            }
        });
    }

    async fn start_snapshot_handler(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut subscriber = self
            .nats_client
            .subscribe("rate_limit_snapshot_request")
            .await?;

        let windows = self.windows.clone();
        let gateway_id = self.gateway_id.clone();
        let nats_client = self.nats_client.clone();

        tokio::spawn(async move {
            info!(gateway_id = %gateway_id, "[SNAPSHOT_HANDLER] Listening");

            while let Some(message) = subscriber.next().await {
                let Ok(request) = serde_json::from_slice::<SnapshotRequest>(&message.payload)
                else {
                    continue;
                };

                if request.requesting_gateway == gateway_id {
                    continue;
                }

                debug!(
                    key = %request.key,
                    requesting_gateway = %request.requesting_gateway,
                    "[SNAPSHOT_HANDLER] Processing snapshot request"
                );

                // Scope: DashMap shard guard lives only inside this block.
                // Holding it across the .await below could stall any writer
                // queued on the same shard (e.g. from get_or_load_window).
                let window = {
                    let Some(window_ref) = windows.get(&request.key) else {
                        continue;
                    };
                    window_ref.value().clone()
                };

                // Scope: read guard lives only across compression. Same hazard
                // as Bug A — holding it across the NATS publish below would
                // stall every writer on this window if NATS slowed.
                let compressed = {
                    let w = window.read().await;
                    match w.to_compressed() {
                        Ok(c) => c,
                        Err(_) => {
                            error!("[SNAPSHOT_HANDLER] Compression error");
                            continue;
                        }
                    }
                };

                let compressed_base64 =
                    base64::engine::general_purpose::STANDARD.encode(&compressed);
                let response = SnapshotResponse {
                    gateway_id: gateway_id.clone(),
                    timestamp_ms: Utc::now().timestamp_millis(),
                    data: compressed_base64,
                };

                let Ok(response_json) = serde_json::to_vec(&response) else {
                    error!("[SNAPSHOT_HANDLER] Serialization error");
                    continue;
                };

                if let Some(reply_subject) = message.reply.as_ref() {
                    if let Err(e) = nats_client
                        .publish(reply_subject.clone(), response_json.into())
                        .await
                    {
                        error!("[SNAPSHOT_HANDLER] Send error: {:?}", e);
                    }
                } else {
                    error!("[SNAPSHOT_HANDLER] No reply subject in request");
                }
            }
        });

        Ok(())
    }
}
