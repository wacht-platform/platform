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
use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};
use tokio::sync::RwLock;

const SECONDS_BUCKETS: usize = 1800; // 30 minutes
const MINUTES_BUCKETS: usize = 1440; // 24 hours
const HOURS_BUCKETS: usize = 24; // 24 hours

#[derive(Clone, Debug, Serialize, Deserialize)]
struct BucketedWindow {
    seconds: Box<[u16]>,
    minutes: Box<[u16]>,
    hours: Box<[u32]>,
    max_requests: u32,
    last_second: i64,
    last_minute: i64,
    last_hour: i64,
}

impl BucketedWindow {
    fn new(max_requests: u32, _window_seconds: i64) -> Self {
        Self {
            seconds: vec![0u16; SECONDS_BUCKETS].into_boxed_slice(),
            minutes: vec![0u16; MINUTES_BUCKETS].into_boxed_slice(),
            hours: vec![0u32; HOURS_BUCKETS].into_boxed_slice(),
            max_requests,
            last_second: 0,
            last_minute: 0,
            last_hour: 0,
        }
    }

    fn rollup_seconds_to_minutes(&mut self, now: i64) {
        let current_minute = now / 60;

        if self.last_minute == 0 {
            self.last_minute = current_minute;
            return;
        }

        if current_minute > self.last_minute {
            for minute in (self.last_minute + 1)..=current_minute {
                let start_ts = (minute - 1) * 60;
                let mut sum = 0u32;

                for i in 0..60 {
                    let ts = start_ts + i;
                    let idx = (ts % SECONDS_BUCKETS as i64) as usize;
                    sum += self.seconds[idx] as u32;
                    self.seconds[idx] = 0;
                }

                let minute_idx = ((minute - 1) % MINUTES_BUCKETS as i64) as usize;
                self.minutes[minute_idx] = sum.min(u16::MAX as u32) as u16;
            }

            self.last_minute = current_minute;
        }
    }

    fn rollup_minutes_to_hours(&mut self, now: i64) {
        let current_hour = now / 3600;

        if self.last_hour == 0 {
            self.last_hour = current_hour;
            return;
        }

        if current_hour > self.last_hour {
            for hour in (self.last_hour + 1)..=current_hour {
                let start_minute = (hour - 1) * 60;
                let mut sum = 0u32;

                for i in 0..60 {
                    let minute = start_minute + i;
                    let idx = (minute % MINUTES_BUCKETS as i64) as usize;
                    sum += self.minutes[idx] as u32;
                    self.minutes[idx] = 0;
                }

                let hour_idx = ((hour - 1) % HOURS_BUCKETS as i64) as usize;
                self.hours[hour_idx] = sum;
            }

            self.last_hour = current_hour;
        }
    }

    fn check_and_add_request(&mut self, window_seconds: i64, limit: u32) -> (bool, u32, u32) {
        let now = Utc::now().timestamp();

        if self.last_second == 0 {
            self.last_second = now;
            self.last_minute = now / 60;
            self.last_hour = now / 3600;
        }

        self.rollup_seconds_to_minutes(now);
        self.rollup_minutes_to_hours(now);

        let (allowed, remaining, retry_after) = self.check_window(window_seconds, limit, now);

        if allowed {
            let second_idx = (now % SECONDS_BUCKETS as i64) as usize;
            if self.seconds[second_idx] < u16::MAX {
                self.seconds[second_idx] += 1;
            }
            self.last_second = now;
        }

        (allowed, remaining, retry_after)
    }

    fn check_window(&self, window_seconds: i64, limit: u32, now: i64) -> (bool, u32, u32) {
        let total = if window_seconds <= 60 {
            self.sum_seconds_only(now, window_seconds)
        } else if window_seconds <= 3600 {
            self.sum_minutes_range(now, window_seconds)
        } else {
            self.sum_hybrid_range(now, window_seconds)
        };

        let allowed = total < limit;
        let remaining = if total < limit { limit - total } else { 0 };
        let retry_after = if !allowed { window_seconds as u32 } else { 0 };

        (allowed, remaining, retry_after)
    }

    fn sum_seconds_only(&self, now: i64, window_seconds: i64) -> u32 {
        let lookback = window_seconds.min(60);
        let mut sum = 0u32;

        let current_second_in_minute = now % 60;
        let seconds_in_current_minute = (current_second_in_minute + 1).min(lookback);
        let seconds_needed_from_prev_minute = lookback - seconds_in_current_minute;

        for i in 0..seconds_in_current_minute {
            let idx = ((now - i) % SECONDS_BUCKETS as i64) as usize;
            sum += self.seconds[idx] as u32;
        }

        if seconds_needed_from_prev_minute > 0 {
            let prev_minute = (now / 60 - 1) % MINUTES_BUCKETS as i64;
            let prev_minute_idx = prev_minute as usize;
            sum += self.minutes[prev_minute_idx] as u32;
        }

        sum
    }

    fn sum_minutes_range(&self, now: i64, window_seconds: i64) -> u32 {
        let current_minute = now / 60;
        let window_minutes = (window_seconds + 59) / 60;
        let lookback_minutes = window_minutes.min(MINUTES_BUCKETS as i64);

        let mut sum = 0u32;

        for i in 1..=lookback_minutes {
            let idx = ((current_minute - i) % MINUTES_BUCKETS as i64) as usize;
            sum += self.minutes[idx] as u32;
        }

        let partial_minute_seconds = now % 60;
        for i in 0..partial_minute_seconds {
            let idx = ((now - i) % SECONDS_BUCKETS as i64) as usize;
            sum += self.seconds[idx] as u32;
        }

        sum
    }

    fn sum_hybrid_range(&self, now: i64, window_seconds: i64) -> u32 {
        let window_start = now - window_seconds;
        let start_hour = (window_start + 3599) / 3600;
        let end_hour = now / 3600;

        let mut sum = 0u32;

        if window_start % 3600 != 0 {
            let next_hour_boundary = start_hour * 3600;
            let partial_start_minutes = (next_hour_boundary - window_start + 59) / 60;

            for i in 0..partial_start_minutes {
                let minute = (window_start / 60) + i;
                let idx = (minute % MINUTES_BUCKETS as i64) as usize;
                sum += self.minutes[idx] as u32;
            }
        }

        for hour in start_hour..end_hour {
            let hour_idx = (hour % HOURS_BUCKETS as i64) as usize;
            sum += self.hours[hour_idx];
        }

        if now % 3600 != 0 {
            let current_hour_start = end_hour * 3600;
            let partial_end_minutes = (now - current_hour_start) / 60;

            for i in 0..partial_end_minutes {
                let minute = (current_hour_start / 60) + i;
                let idx = (minute % MINUTES_BUCKETS as i64) as usize;
                sum += self.minutes[idx] as u32;
            }

            let partial_minute_seconds = now % 60;
            if partial_minute_seconds > 0 {
                for i in 0..partial_minute_seconds {
                    let idx = ((now - i) % SECONDS_BUCKETS as i64) as usize;
                    sum += self.seconds[idx] as u32;
                }
            }
        }

        sum
    }

    fn apply_delta(&mut self, timestamp: i64) {
        let now = Utc::now().timestamp();

        if self.last_second == 0 {
            self.last_second = now;
            self.last_minute = now / 60;
            self.last_hour = now / 3600;
        }

        self.rollup_seconds_to_minutes(now);
        self.rollup_minutes_to_hours(now);

        let second_idx = (timestamp % SECONDS_BUCKETS as i64) as usize;
        if self.seconds[second_idx] < u16::MAX {
            self.seconds[second_idx] += 1;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeltaMessage {
    key: String,
    timestamp: i64,
    node_id: String,
}

#[derive(Clone, Debug)]
struct CachedApiKeyData {
    data: ApiKeyGatewayData,
    cached_at: DateTime<Utc>,
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

        let bucket = match jetstream
            .create_key_value(kv::Config {
                bucket: "rate_limits_bucketed".to_string(),
                description: "Bucketed rate limiter storage".to_string(),
                max_age: Duration::from_secs(90000),
                storage: StorageType::File,
                ..Default::default()
            })
            .await
        {
            Ok(bucket) => bucket,
            Err(_) => jetstream.get_key_value("rate_limits_bucketed").await?,
        };

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
    api_key_cache: Arc<Cache<String, Arc<CachedApiKeyData>>>,
    storage: Arc<NatsKvStorage>,
    delta_publisher: Arc<RwLock<Option<async_nats::Client>>>,
    dirty_keys: Arc<RwLock<HashSet<String>>>,
}

impl RateLimiter {
    async fn new(nats_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let storage = Arc::new(NatsKvStorage::new(nats_url).await?);

        let cache = Cache::builder()
            .time_to_idle(Duration::from_secs(3600))
            .max_capacity(100_000)
            .build();

        let api_key_cache = Cache::builder()
            .time_to_live(Duration::from_secs(60))
            .max_capacity(100_000)
            .build();

        let limiter = Self {
            cache: Arc::new(cache),
            api_key_cache: Arc::new(api_key_cache),
            storage: storage.clone(),
            delta_publisher: Arc::new(RwLock::new(None)),
            dirty_keys: Arc::new(RwLock::new(HashSet::new())),
        };

        limiter.start_distributed_sync().await?;
        limiter.start_persistence_worker();

        Ok(limiter)
    }

    async fn get_api_key_data(
        &self,
        key_hash: String,
        app_state: &AppState,
    ) -> Result<ApiKeyGatewayData, Response> {
        if let Some(cached) = self.api_key_cache.get(&key_hash).await {
            let age = Utc::now() - cached.cached_at;

            if age.num_seconds() < 60 {
                return Ok(cached.data.clone());
            }

            let app_state_clone = app_state.clone();
            let key_hash_clone = key_hash.clone();
            let api_key_cache_clone = self.api_key_cache.clone();

            tokio::spawn(async move {
                if let Ok(Some(fresh_data)) = GetApiKeyGatewayDataQuery::new(key_hash_clone.clone())
                    .execute(&app_state_clone)
                    .await
                {
                    let cached_data = Arc::new(CachedApiKeyData {
                        data: fresh_data,
                        cached_at: Utc::now(),
                    });
                    api_key_cache_clone.insert(key_hash_clone, cached_data).await;
                }
            });

            return Ok(cached.data.clone());
        }

        match GetApiKeyGatewayDataQuery::new(key_hash.clone())
            .execute(app_state)
            .await
        {
            Ok(Some(data)) => {
                let cached_data = Arc::new(CachedApiKeyData {
                    data: data.clone(),
                    cached_at: Utc::now(),
                });
                self.api_key_cache.insert(key_hash, cached_data).await;
                Ok(data)
            }
            Ok(None) => Err((StatusCode::UNAUTHORIZED, "Invalid API key").into_response()),
            Err(_) => Err((StatusCode::INTERNAL_SERVER_ERROR, "Database error").into_response()),
        }
    }

    async fn start_distributed_sync(&self) -> Result<(), Box<dyn std::error::Error>> {
        let client = self.storage.client.clone();
        let node_id = self.storage.node_id.clone();

        *self.delta_publisher.write().await = Some(client.clone());

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
                            window.apply_delta(delta.timestamp);
                        }
                    }
                }
            }
        });

        Ok(())
    }

    fn start_persistence_worker(&self) {
        let storage = self.storage.clone();
        let cache = self.cache.clone();
        let dirty_keys = self.dirty_keys.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;

                let keys_to_persist: Vec<String> = {
                    let mut dirty = dirty_keys.write().await;
                    let keys: Vec<String> = dirty.drain().collect();
                    keys
                };

                for key in keys_to_persist {
                    if let Some(window_lock) = cache.get(&key).await {
                        let window = window_lock.read().await.clone();

                        for attempt in 0..3 {
                            match storage.get_with_revision(&key).await {
                                Ok(Some((_, revision))) => {
                                    if storage.update_cas(&key, &window, revision).await.is_ok() {
                                        break;
                                    }
                                }
                                Ok(None) => {
                                    let _ = storage
                                        .bucket
                                        .put(&key, serde_json::to_vec(&window).unwrap().into())
                                        .await;
                                    break;
                                }
                                Err(_) => {
                                    if attempt == 2 {
                                        let mut dirty = dirty_keys.write().await;
                                        dirty.insert(key.clone());
                                    }
                                }
                            }
                            tokio::time::sleep(Duration::from_millis(10)).await;
                        }
                    }
                }
            }
        });
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

            bucketed_window.check_and_add_request(window, limit)
        };

        if allowed {
            self.publish_delta(&key, now).await;

            let mut dirty = self.dirty_keys.write().await;
            dirty.insert(key);
        }

        (allowed, remaining, retry_after)
    }
}

fn is_valid_api_key_format(key: &str) -> bool {
    if key.len() < 45 || key.len() > 100 {
        return false;
    }

    if let Some(underscore_pos) = key.find('_') {
        if underscore_pos == 0 {
            return false;
        }

        let secret_part = &key[underscore_pos + 1..];

        if secret_part.len() != 43 {
            return false;
        }

        secret_part.chars().all(|c| {
            c.is_ascii_alphanumeric() || c == '-' || c == '_'
        })
    } else {
        false
    }
}

async fn check_limit(
    Path(identifier): Path<String>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    State((limiter, app_state)): State<(RateLimiter, AppState)>,
) -> Response {
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

    if !is_valid_api_key_format(api_key) {
        return (StatusCode::UNAUTHORIZED, "Invalid API key format").into_response();
    }

    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let key_hash = format!("{:x}", hasher.finalize());

    let key_data = match limiter.get_api_key_data(key_hash, &app_state).await {
        Ok(data) => data,
        Err(response) => return response,
    };

    if let Some(expires_at) = key_data.expires_at {
        if expires_at < Utc::now() {
            return (StatusCode::UNAUTHORIZED, "API key has expired").into_response();
        }
    }

    let client_ip = headers
        .get("x-original-client-ip")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| addr.ip().to_string());

    let mut response_headers = HeaderMap::new();

    // API Key identity headers
    response_headers.insert(
        "X-Wacht-Key-ID",
        HeaderValue::from_str(&key_data.key_id.to_string()).unwrap(),
    );
    response_headers.insert(
        "X-Wacht-Deployment-ID",
        HeaderValue::from_str(&key_data.deployment_id.to_string()).unwrap(),
    );
    response_headers.insert(
        "X-Wacht-App-ID",
        HeaderValue::from_str(&key_data.app_id.to_string()).unwrap(),
    );
    response_headers.insert(
        "X-Wacht-App-Name",
        HeaderValue::from_str(&key_data.app_name).unwrap(),
    );
    response_headers.insert(
        "X-Wacht-Key-Name",
        HeaderValue::from_str(&key_data.key_name).unwrap(),
    );

    // Permissions as JSON array
    if let Ok(permissions_json) = serde_json::to_string(&key_data.permissions) {
        if let Ok(header_value) = HeaderValue::from_str(&permissions_json) {
            response_headers.insert("X-Wacht-Permissions", header_value);
        }
    }

    // Metadata as JSON
    if let Ok(metadata_json) = serde_json::to_string(&key_data.metadata) {
        if let Ok(header_value) = HeaderValue::from_str(&metadata_json) {
            response_headers.insert("X-Wacht-Metadata", header_value);
        }
    }

    let mut all_allowed = true;
    let mut min_retry_after = u32::MAX;

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

    if limits.is_empty() {
        limits.push((100, 60, RateLimitMode::PerKey));
    }

    for (limit, window, rate_limit_mode) in limits.iter() {
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

    let app_state = AppState::new_from_env().await?;

    println!("Connecting to NATS at: {}", nats_url);

    let rate_limiter = RateLimiter::new(&nats_url).await?;

    let app = Router::new()
        .route("/check/:identifier", get(check_limit))
        .route("/health", get(health))
        .with_state((rate_limiter, app_state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3002").await?;

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;

    Ok(())
}
