use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitCounter {
    pub key: String,
    pub count: i64,
    pub last_updated: DateTime<Utc>,
    pub max_requests: i32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitCounterInput {
    pub key: String,
    pub count: i64,
    pub last_updated: DateTime<Utc>,
    pub max_requests: i32,
}
