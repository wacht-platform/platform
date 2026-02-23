// Time constants for rate limiting buckets
// Using readable constants instead of magic numbers

/// Milliseconds per second
pub const MS_PER_SECOND: i64 = 1_000;

/// Milliseconds per minute
pub const MS_PER_MINUTE: i64 = 60_000;

/// Milliseconds per hour  
pub const MS_PER_HOUR: i64 = 3_600_000;

/// Milliseconds per day
pub const MS_PER_DAY: i64 = 86_400_000;

/// Seconds per minute
pub const SECS_PER_MINUTE: i64 = 60;

/// Seconds per hour
pub const SECS_PER_HOUR: i64 = 3_600;

/// Seconds per day
pub const SECS_PER_DAY: i64 = 86_400;

/// Minutes per hour
pub const MINS_PER_HOUR: i64 = 60;

/// Hours per day
pub const HOURS_PER_DAY: i64 = 24;

// Bucket array sizes
/// Number of millisecond buckets (tracks last 120ms)
pub const MS_BUCKETS: usize = 120;

/// Number of 100ms buckets (tracks last 9 seconds)
pub const HUNDRED_MS_BUCKETS: usize = 90;

/// Number of second buckets (tracks last hour)
pub const SECONDS_BUCKETS: usize = 3600;

/// Number of minute buckets (tracks last 2 hours)
pub const MINUTES_BUCKETS: usize = 120;

/// Number of hour buckets (tracks last 2 days)
pub const HOURS_BUCKETS: usize = 48;

/// Number of day buckets (tracks last 30 days)
pub const DAYS_BUCKETS: usize = 30;
