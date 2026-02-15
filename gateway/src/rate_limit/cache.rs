// Cache types for rate limiting
// Stores cached API key and rate limit scheme data

use chrono::{DateTime, Utc};
use models::api_key::RateLimit;
use queries::api_key_gateway::ApiKeyGatewayData;

/// Error type for cache lookups
#[derive(Debug, Clone)]
pub enum CacheLookupError {
    /// The requested item was not found in the database
    NotFound,
    /// A database error occurred during lookup
    DatabaseError,
}

impl std::fmt::Display for CacheLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "not found"),
            Self::DatabaseError => write!(f, "database error"),
        }
    }
}

impl std::error::Error for CacheLookupError {}

/// Cached API key data with timestamp
#[derive(Clone, Debug)]
pub struct CachedApiKeyData {
    pub data: ApiKeyGatewayData,
    pub cached_at: DateTime<Utc>,
}

/// Cached rate limit scheme data with timestamp
#[derive(Clone, Debug)]
pub struct CachedRateLimitSchemeData {
    pub data: Vec<RateLimit>,
    pub cached_at: DateTime<Utc>,
}
