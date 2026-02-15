// Gateway library - rate limiting with distributed sync

use common::state::AppState;

pub mod delta_stream;
pub mod handlers;
pub mod rate_limit;
pub mod validation;

// Re-export main types for backward compatibility
pub use delta_stream::{DeltaPublisher, RateLimitDelta};
pub use rate_limit::{BucketedWindow, RateLimiter};
pub use validation::*;

/// Wrapper for gateway state
#[derive(Clone)]
pub struct GatewayState {
    pub rate_limiter: RateLimiter,
    pub app_state: AppState,
}
