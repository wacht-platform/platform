use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A single rate limit state for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitState {
    pub rule: String,
    pub remaining: i32,
    pub limit: i32,
}

/// API key verification event for Tinybird audit logs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyVerificationEvent {
    pub request_id: String,
    pub deployment_id: i64,
    pub app_slug: String,
    pub key_id: i64,
    pub key_name: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by_rule: Option<String>,
    pub client_ip: String,
    pub path: String,
    pub user_agent: String,
    pub rate_limits: String,
    pub latency_us: i64,
    pub timestamp: DateTime<Utc>,
}

impl ApiKeyVerificationEvent {
    pub fn new(
        request_id: String,
        deployment_id: i64,
        app_slug: String,
        key_id: i64,
        key_name: String,
        outcome: String,
        client_ip: String,
        path: String,
        user_agent: String,
    ) -> Self {
        Self {
            request_id,
            deployment_id,
            app_slug,
            key_id,
            key_name,
            outcome,
            blocked_by_rule: None,
            client_ip,
            path,
            user_agent,
            rate_limits: "[]".to_string(),
            latency_us: 0,
            timestamp: Utc::now(),
        }
    }

    pub fn with_blocked_by(mut self, rule: String) -> Self {
        self.blocked_by_rule = Some(rule);
        self
    }

    pub fn with_rate_limits(mut self, limits: Vec<RateLimitState>) -> Self {
        self.rate_limits = serde_json::to_string(&limits).unwrap_or_else(|_| "[]".to_string());
        self
    }

    pub fn with_latency(mut self, latency_us: i64) -> Self {
        self.latency_us = latency_us;
        self
    }
}
