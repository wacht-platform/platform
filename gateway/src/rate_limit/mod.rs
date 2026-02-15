// Rate limiting module
// Provides bucketed window rate limiting with distributed synchronization

pub mod cache;
pub mod service;
pub mod sync;
pub mod time;
pub mod window;

// Re-export main types for convenience
pub use cache::{CacheLookupError, CachedApiKeyData, CachedRateLimitSchemeData};
pub use service::RateLimiter;
pub use time::*;
pub use window::BucketedWindow;
