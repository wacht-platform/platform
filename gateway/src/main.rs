use async_nats::jetstream::{self, kv, stream::StorageType};
use axum::{
    Router,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, Utc};
use common::state::AppState;
use dotenvy::dotenv;
use futures::StreamExt;
use models::api_key::RateLimitMode;
use moka::future::Cache;
use queries::{
    Query as QueryTrait,
    api_key_gateway::{ApiKeyGatewayData, GetApiKeyGatewayDataQuery},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::RwLock;

const BITS_PER_BUCKET: u32 = 16;
const BUCKETS_PER_U64: usize = 4;
const MAX_BUCKET_VALUE: u32 = 0xFFFF;
const BUCKET_MASK: u64 = 0xFFFF;

const SECONDS_BUCKETS: usize = 60;
const MINUTES_BUCKETS: usize = 60;
const HOURS_BUCKETS: usize = 24;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BucketedWindow {
    seconds: Box<[u64]>,
    minutes: Box<[u64]>,
    hours: Box<[u64]>,
    max_requests: u32,
    last_second: i64,
    last_minute: i64,
    last_hour: i64,
}

impl BucketedWindow {
    fn new(max_requests: u32, _window_seconds: i64) -> Self {
        let seconds_u64s = (SECONDS_BUCKETS + BUCKETS_PER_U64 - 1) / BUCKETS_PER_U64;
        let minutes_u64s = (MINUTES_BUCKETS + BUCKETS_PER_U64 - 1) / BUCKETS_PER_U64;
        let hours_u64s = (HOURS_BUCKETS + BUCKETS_PER_U64 - 1) / BUCKETS_PER_U64;

        Self {
            seconds: vec![0u64; seconds_u64s].into_boxed_slice(),
            minutes: vec![0u64; minutes_u64s].into_boxed_slice(),
            hours: vec![0u64; hours_u64s].into_boxed_slice(),
            max_requests,
            last_second: 0,
            last_minute: 0,
            last_hour: 0,
        }
    }

    fn get_bucket(buffer: &[u64], index: usize) -> u32 {
        let word_index = index / BUCKETS_PER_U64;
        let bit_offset = (index % BUCKETS_PER_U64) * BITS_PER_BUCKET as usize;
        ((buffer[word_index] >> bit_offset) & BUCKET_MASK) as u32
    }

    fn set_bucket(buffer: &mut [u64], index: usize, value: u32) {
        let word_index = index / BUCKETS_PER_U64;
        let bit_offset = (index % BUCKETS_PER_U64) * BITS_PER_BUCKET as usize;

        buffer[word_index] &= !(BUCKET_MASK << bit_offset);
        buffer[word_index] |= ((value as u64) & BUCKET_MASK) << bit_offset;
    }

    fn increment_bucket(buffer: &mut [u64], index: usize) {
        let current = Self::get_bucket(buffer, index);
        if current < MAX_BUCKET_VALUE {
            Self::set_bucket(buffer, index, current + 1);
        }
    }

    fn rollup_seconds_to_minutes(&mut self, now: i64) {
        let current_minute = now / 60;
        if current_minute > self.last_minute {
            let mut sum = 0u32;
            for i in 0..SECONDS_BUCKETS {
                sum += Self::get_bucket(&self.seconds, i);
            }

            let minute_idx = (current_minute % MINUTES_BUCKETS as i64) as usize;
            Self::set_bucket(&mut self.minutes, minute_idx, sum);

            self.seconds.fill(0);
            self.last_minute = current_minute;
        }
    }

    fn rollup_minutes_to_hours(&mut self, now: i64) {
        let current_hour = now / 3600;
        if current_hour > self.last_hour {
            let mut sum = 0u32;
            for i in 0..MINUTES_BUCKETS {
                sum += Self::get_bucket(&self.minutes, i);
            }

            let hour_idx = (current_hour % HOURS_BUCKETS as i64) as usize;
            Self::set_bucket(&mut self.hours, hour_idx, sum);

            self.minutes.fill(0);
            self.last_hour = current_hour;
        }
    }

    fn try_add_request(&mut self) -> (bool, u32) {
        let now = Utc::now().timestamp();

        if self.last_second == 0 {
            self.last_second = now;
            self.last_minute = now / 60;
            self.last_hour = now / 3600;
        }

        self.rollup_seconds_to_minutes(now);
        self.rollup_minutes_to_hours(now);

        let second_idx = (now % SECONDS_BUCKETS as i64) as usize;
        Self::increment_bucket(&mut self.seconds, second_idx);

        self.last_second = now;

        (true, 0)
    }

    fn check_window(&self, window_seconds: i64, limit: u32) -> (bool, u32, u32) {
        let now = Utc::now().timestamp();

        let total = if window_seconds <= 60 {
            let lookback = window_seconds.min(SECONDS_BUCKETS as i64);
            let mut sum = 0u32;
            for i in 0..lookback as usize {
                let idx = ((now - i as i64) % SECONDS_BUCKETS as i64) as usize;
                sum += Self::get_bucket(&self.seconds, idx);
            }
            sum
        } else if window_seconds <= 3600 {
            let lookback_minutes = (window_seconds / 60).min(MINUTES_BUCKETS as i64);
            let mut sum = 0u32;
            for i in 0..lookback_minutes as usize {
                let idx = ((now / 60 - i as i64) % MINUTES_BUCKETS as i64) as usize;
                sum += Self::get_bucket(&self.minutes, idx);
            }

            let partial_minute_seconds = now % 60;
            for i in 0..partial_minute_seconds as usize {
                let idx = ((now - i as i64) % SECONDS_BUCKETS as i64) as usize;
                sum += Self::get_bucket(&self.seconds, idx);
            }
            sum
        } else {
            let lookback_hours = (window_seconds / 3600).min(HOURS_BUCKETS as i64);
            let mut sum = 0u32;
            for i in 0..lookback_hours as usize {
                let idx = ((now / 3600 - i as i64) % HOURS_BUCKETS as i64) as usize;
                sum += Self::get_bucket(&self.hours, idx);
            }

            let partial_hour_minutes = (now % 3600) / 60;
            for i in 0..partial_hour_minutes as usize {
                let idx = ((now / 60 - i as i64) % MINUTES_BUCKETS as i64) as usize;
                sum += Self::get_bucket(&self.minutes, idx);
            }

            let partial_minute_seconds = now % 60;
            for i in 0..partial_minute_seconds as usize {
                let idx = ((now - i as i64) % SECONDS_BUCKETS as i64) as usize;
                sum += Self::get_bucket(&self.seconds, idx);
            }
            sum
        };

        let allowed = total < limit;
        let remaining = if total < limit { limit - total } else { 0 };
        let retry_after = if !allowed { window_seconds as u32 } else { 0 };

        (allowed, remaining, retry_after)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeltaMessage {
    key: String,
    timestamp: i64,
    node_id: String,
}

struct NatsKvStorage {
    bucket: kv::Store,
    client: async_nats::Client,
    node_id: String,
}

impl NatsKvStorage {
    async fn new(nats_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let client = async_nats::connect(nats_url).await?;
        let jetstream = jetstream::new(client.clone());

        // Create or get the KV bucket for rate limits
        let bucket = match jetstream
            .create_key_value(kv::Config {
                bucket: "rate_limits_bucketed".to_string(),
                description: "Bucketed rate limiter storage".to_string(),
                max_age: Duration::from_secs(90000), // 25 hours TTL
                storage: StorageType::File,
                ..Default::default()
            })
            .await
        {
            Ok(bucket) => bucket,
            Err(_) => jetstream.get_key_value("rate_limits_bucketed").await?,
        };

        // Generate unique node ID
        let node_id = format!("node_{}", std::process::id());

        Ok(Self {
            bucket,
            client,
            node_id,
        })
    }

    async fn update_cas(
        &self,
        key: &str,
        window: &BucketedWindow,
        revision: u64,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        let json = serde_json::to_vec(window)?;
        let new_revision = self.bucket.update(key, json.into(), revision).await?;
        Ok(new_revision)
    }

    async fn get_with_revision(
        &self,
        key: &str,
    ) -> Result<Option<(BucketedWindow, u64)>, Box<dyn std::error::Error + Send + Sync>> {
        match self.bucket.entry(key).await {
            Ok(Some(entry)) => {
                let window: BucketedWindow = serde_json::from_slice(&entry.value)?;
                Ok(Some((window, entry.revision)))
            }
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    async fn load(
        &self,
        key: &str,
    ) -> Result<Option<BucketedWindow>, Box<dyn std::error::Error + Send + Sync>> {
        match self.bucket.get(key).await {
            Ok(Some(entry)) => {
                let window: BucketedWindow = serde_json::from_slice(&entry)?;
                Ok(Some(window))
            }
            Ok(None) => Ok(None),
            Err(_) => Ok(None),
        }
    }
}

#[derive(Clone)]
struct RateLimiter {
    cache: Arc<Cache<String, Arc<RwLock<BucketedWindow>>>>,
    storage: Arc<NatsKvStorage>,
    delta_publisher: Arc<RwLock<Option<async_nats::Client>>>,
}

impl RateLimiter {
    async fn new(nats_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let storage = Arc::new(NatsKvStorage::new(nats_url).await?);

        // Cache with 1 hour TTL for active entries
        let cache = Cache::builder()
            .time_to_idle(Duration::from_secs(3600)) // 1 hour
            .max_capacity(100_000)
            .build();

        let limiter = Self {
            cache: Arc::new(cache),
            storage: storage.clone(),
            delta_publisher: Arc::new(RwLock::new(None)),
        };

        // Start background tasks for distributed sync
        limiter.start_distributed_sync().await?;

        Ok(limiter)
    }

    async fn start_distributed_sync(&self) -> Result<(), Box<dyn std::error::Error>> {
        let client = self.storage.client.clone();
        let node_id = self.storage.node_id.clone();

        // Store the client for publishing
        *self.delta_publisher.write().await = Some(client.clone());

        // Subscribe to delta stream for bucket updates
        let cache = self.cache.clone();
        let node_id_for_delta = node_id.clone();
        let delta_sub = client.subscribe("rate_limiter.delta.*").await?;
        tokio::spawn(async move {
            let mut sub = delta_sub;
            while let Some(msg) = sub.next().await {
                if let Ok(delta) = serde_json::from_slice::<DeltaMessage>(&msg.payload) {
                    if delta.node_id != node_id_for_delta {
                        if let Some(window_lock) = cache.get(&delta.key).await {
                            let mut window = window_lock.write().await;
                            let second_idx = (delta.timestamp % SECONDS_BUCKETS as i64) as usize;
                            BucketedWindow::increment_bucket(&mut window.seconds, second_idx);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn publish_delta(&self, key: &str, timestamp: i64) {
        if let Some(client) = self.delta_publisher.read().await.as_ref() {
            let delta = DeltaMessage {
                key: key.to_string(),
                timestamp,
                node_id: self.storage.node_id.clone(),
            };

            if let Ok(payload) = serde_json::to_vec(&delta) {
                let subject = format!("rate_limiter.delta.{}", self.storage.node_id);
                let _ = client.publish(subject, payload.into()).await;
            }
        }
    }

    async fn check_rate_limit(&self, key: String, limit: u32, window: i64) -> (bool, u32, u32) {
        let window_lock = if let Some(cached_lock) = self.cache.get(&key).await {
            cached_lock
        } else {
            let loaded_window = match self.storage.load(&key).await {
                Ok(Some(stored)) => stored,
                _ => BucketedWindow::new(limit, window),
            };

            let lock = Arc::new(RwLock::new(loaded_window));
            self.cache.insert(key.clone(), lock.clone()).await;
            lock
        };

        let now = Utc::now().timestamp();

        let (allowed, remaining, retry_after) = {
            let mut bucketed_window = window_lock.write().await;

            if bucketed_window.max_requests != limit {
                bucketed_window.max_requests = limit;
            }

            bucketed_window.try_add_request();

            bucketed_window.check_window(window, limit)
        };

        if allowed {
            self.publish_delta(&key, now).await;

            let storage = self.storage.clone();
            let key_clone = key.clone();
            tokio::spawn(async move {
                Self::persist_async(storage, key_clone).await;
            });
        }

        (allowed, remaining, retry_after)
    }

    async fn persist_async(storage: Arc<NatsKvStorage>, key: String) {
        for _ in 0..5 {
            match storage.get_with_revision(&key).await {
                Ok(Some((window, revision))) => {
                    if storage.update_cas(&key, &window, revision).await.is_ok() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(_) => return,
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

async fn check_limit(
    Path(identifier): Path<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State((limiter, app_state)): State<(RateLimiter, AppState)>,
) -> Response {
    // Extract API key from headers
    let api_key = headers
        .get("x-api-key")
        .or_else(|| headers.get("authorization"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer ").or(Some(v)));

    let api_key = match api_key {
        Some(key) => key,
        None => {
            return (StatusCode::UNAUTHORIZED, "API key required").into_response();
        }
    };

    // Hash the API key
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    // Get API key data with rate limits in a single query
    let key_data = match GetApiKeyGatewayDataQuery::new(key_hash)
        .execute(&app_state)
        .await
    {
        Ok(Some(data)) => data,
        Ok(None) => {
            return (StatusCode::UNAUTHORIZED, "Invalid API key").into_response();
        }
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response();
        }
    };

    // Check expiration
    if let Some(expires_at) = key_data.expires_at {
        if expires_at < Utc::now() {
            return (StatusCode::UNAUTHORIZED, "API key has expired").into_response();
        }
    }

    // Get client IP - prefer custom header from user's backend, fall back to connection IP
    // X-Original-Client-IP is the header that the user's backend should set with their end-user's IP
    let client_ip = headers
        .get("x-original-client-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| {
            // Fall back to direct connection IP if custom header not present
            // This would be the user's backend server IP if they didn't set the header
            addr.ip().to_string()
        });

    let mut response_headers = HeaderMap::new();
    response_headers.insert(
        "X-Deployment-ID",
        HeaderValue::from_str(&key_data.deployment_id.to_string()).unwrap(),
    );

    let mut all_allowed = true;
    let mut min_retry_after = u32::MAX;

    // Build rate limit checks based on configured limits
    let mut limits = Vec::new();
    for rate_limit in &key_data.rate_limits {
        let window_seconds = rate_limit.window_seconds();
        let rate_limit_mode = rate_limit.effective_mode();
        limits.push((
            rate_limit.max_requests as u32,
            window_seconds,
            rate_limit_mode,
        ));
    }

    // If no limits configured, use a default
    if limits.is_empty() {
        limits.push((100, 60, RateLimitMode::PerKey));
    }

    // Check all rate limits
    for (limit, window, rate_limit_mode) in limits.iter() {
        // Build the rate limit key based on the mode
        let key = match rate_limit_mode {
            RateLimitMode::PerKey => {
                format!("key:{}:{}:{}", key_data.key_id, identifier, window)
            }
            RateLimitMode::PerIp => {
                format!(
                    "ip:{}:{}:{}:{}",
                    key_data.deployment_id, client_ip, identifier, window
                )
            }
            RateLimitMode::PerKeyAndIp => {
                format!(
                    "key_ip:{}:{}:{}:{}",
                    key_data.key_id, client_ip, identifier, window
                )
            }
        };

        let (allowed, remaining, retry_after) =
            limiter.check_rate_limit(key, *limit, *window).await;

        let limit_header = format!("X-RateLimit-{}s-Limit", window);
        let remaining_header = format!("X-RateLimit-{}s-Remaining", window);
        response_headers.insert(
            axum::http::HeaderName::from_bytes(limit_header.as_bytes()).unwrap(),
            HeaderValue::from_str(&limit.to_string()).unwrap(),
        );
        response_headers.insert(
            axum::http::HeaderName::from_bytes(remaining_header.as_bytes()).unwrap(),
            HeaderValue::from_str(&remaining.to_string()).unwrap(),
        );

        if !allowed {
            all_allowed = false;
            min_retry_after = min_retry_after.min(retry_after);
            let reset_header = format!("X-RateLimit-{}s-Reset", window);
            response_headers.insert(
                axum::http::HeaderName::from_bytes(reset_header.as_bytes()).unwrap(),
                HeaderValue::from_str(&retry_after.to_string()).unwrap(),
            );
        }
    }

    if all_allowed {
        (StatusCode::OK, response_headers).into_response()
    } else {
        response_headers.insert(
            "Retry-After",
            HeaderValue::from_str(&min_retry_after.to_string()).unwrap(),
        );
        (StatusCode::TOO_MANY_REQUESTS, response_headers).into_response()
    }
}

async fn health() -> &'static str {
    "OK"
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let nats_url =
        std::env::var("NATS_URL").unwrap_or_else(|_| "nats://localhost:4222".to_string());

    // Initialize app state from environment
    let app_state = AppState::new_from_env().await?;

    println!("Connecting to NATS at: {}", nats_url);

    let rate_limiter = RateLimiter::new(&nats_url).await?;

    let app = Router::new()
        .route("/check/:identifier", get(check_limit))
        .route("/health", get(health))
        .with_state((rate_limiter, app_state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002").await?;

    println!("API Gateway listening on 0.0.0.0:3002");
    println!("Using header 'X-Original-Client-IP' for client IP forwarding");

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
