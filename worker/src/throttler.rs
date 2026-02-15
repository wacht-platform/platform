use anyhow::Result;
use chrono::Utc;
use redis::{Client, Script};

const THROTTLE_LUA_SCRIPT: &str = r#"
local key = KEYS[1]
local now_ms = tonumber(ARGV[1])
local window_ms = tonumber(ARGV[2])
local limit = tonumber(ARGV[3])
local request_id = ARGV[4]
local expire_seconds = tonumber(ARGV[5])

local cutoff = now_ms - window_ms

-- Remove old entries outside the window
redis.call('ZREMRANGEBYSCORE', key, 0, cutoff)

-- Count current entries in window
local count = redis.call('ZCARD', key)

if count < limit then
    -- Add new entry
    redis.call('ZADD', key, now_ms, request_id)
    -- Set expiry to window duration + 1 second buffer
    redis.call('EXPIRE', key, expire_seconds)
    return 1  -- ALLOWED
else
    -- Get oldest timestamp to calculate next slot
    local oldest = redis.call('ZRANGE', key, 0, 0, 'WITHSCORES')
    if #oldest > 0 then
        return tonumber(oldest[2]) + window_ms  -- Return next available time
    else
        return now_ms  -- Shouldn't happen, but fallback
    end
end
"#;

pub struct WebhookThrottler {
    redis_client: Client,
    script: Script,
}

impl WebhookThrottler {
    pub fn new(redis_client: Client) -> Self {
        Self {
            redis_client,
            script: Script::new(THROTTLE_LUA_SCRIPT),
        }
    }

    /// Check if request is allowed and record it
    /// Returns:
    /// - Ok(None) if allowed (no delay needed)
    /// - Ok(Some(delay_ms)) if rate limited (with calculated delay until next slot)
    pub async fn check_and_record(
        &self,
        endpoint_id: i64,
        duration_ms: i64,
        max_requests: i32,
    ) -> Result<Option<i64>> {
        let key = format!("wh:t:{}", endpoint_id);
        let now_ms = Utc::now().timestamp_millis();
        let request_id = format!("{}:{}", now_ms, now_ms % 1000000);

        // Calculate Redis key expiry (window duration + 1 second buffer)
        let expire_seconds = ((duration_ms / 1000) + 1) as usize;

        let mut conn = self.redis_client.get_multiplexed_async_connection().await?;

        let result: i64 = self
            .script
            .key(&key)
            .arg(now_ms)
            .arg(duration_ms)
            .arg(max_requests)
            .arg(&request_id)
            .arg(expire_seconds)
            .invoke_async(&mut conn)
            .await?;

        if result == 1 {
            Ok(None) // Allowed
        } else {
            // Rate limited - calculate delay
            let next_slot_ms = result;
            let delay_ms = (next_slot_ms - now_ms).max(100); // minimum 100ms
            Ok(Some(delay_ms))
        }
    }
}
