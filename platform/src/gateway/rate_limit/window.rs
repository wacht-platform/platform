// Bucketed window for rate limiting
// Stores request counts across multiple time granularities with automatic rollup

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::time::{
    DAYS_BUCKETS, HOURS_BUCKETS, HOURS_PER_DAY, HUNDRED_MS_BUCKETS, MINS_PER_HOUR, MINUTES_BUCKETS,
    MS_BUCKETS, MS_PER_DAY, MS_PER_HOUR, MS_PER_MINUTE, MS_PER_SECOND, SECONDS_BUCKETS,
    SECS_PER_DAY, SECS_PER_HOUR, SECS_PER_MINUTE,
};

/// Bucketed window for tracking request counts across time granularities.
/// Uses automatic rollup from fine (ms) to coarse (days) buckets.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BucketedWindow {
    pub ms: Box<[u16]>,
    pub hundred_ms: Box<[u16]>,
    pub seconds: Box<[u16]>,
    pub minutes: Box<[u16]>,
    pub hours: Box<[u32]>, // u32 to support 100K+ requests/hour
    pub days: Box<[u32]>,  // u32 to support millions of requests/day

    pub last_hundred_ms: i64,
    pub last_second: i64,
    pub last_minute: i64,
    pub last_hour: i64,
    pub last_day: i64,
}

impl Default for BucketedWindow {
    fn default() -> Self {
        Self {
            ms: vec![0u16; MS_BUCKETS].into_boxed_slice(),
            hundred_ms: vec![0u16; HUNDRED_MS_BUCKETS].into_boxed_slice(),
            seconds: vec![0u16; SECONDS_BUCKETS].into_boxed_slice(),
            minutes: vec![0u16; MINUTES_BUCKETS].into_boxed_slice(),
            hours: vec![0u32; HOURS_BUCKETS].into_boxed_slice(),
            days: vec![0u32; DAYS_BUCKETS].into_boxed_slice(),
            last_hundred_ms: 0,
            last_second: 0,
            last_minute: 0,
            last_hour: 0,
            last_day: 0,
        }
    }
}

impl BucketedWindow {
    /// Creates a new empty bucketed window
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Compress the window for storage/transmission
    pub fn to_compressed(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let serialized = bincode::serialize(self)?;
        Ok(zstd::encode_all(&serialized[..], 3)?)
    }

    /// Decompress a window from storage
    pub fn from_compressed(data: &[u8]) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let decompressed = zstd::decode_all(data)?;
        Ok(bincode::deserialize(&decompressed)?)
    }

    /// Check if request is allowed and add it if so.
    /// Returns (allowed, remaining, retry_after_ms)
    pub fn check_and_add_request(
        &mut self,
        window_ms: i64,
        limit: u32,
        is_calendar_day: bool,
    ) -> (bool, u32, u32) {
        let now_ms = Utc::now().timestamp_millis();
        self.rollup_all(now_ms);

        let (allowed, _, retry_after) =
            self.check_window(window_ms, limit, now_ms, is_calendar_day);
        let ms_idx = (now_ms % MS_BUCKETS as i64) as usize;

        if allowed {
            self.ms[ms_idx] = self.ms[ms_idx].saturating_add(1);

            // Recalculate remaining after adding the request
            let (_, remaining, _) = self.check_window(window_ms, limit, now_ms, is_calendar_day);
            return (allowed, remaining, retry_after);
        }

        let (_, remaining, _) = self.check_window(window_ms, limit, now_ms, is_calendar_day);
        (allowed, remaining, retry_after)
    }

    /// Apply a delta from another gateway instance
    #[inline]
    pub fn apply_delta(&mut self, timestamp_ms: i64) {
        self.rollup_all(timestamp_ms);
        let ms_idx = (timestamp_ms % MS_BUCKETS as i64) as usize;
        self.ms[ms_idx] = self.ms[ms_idx].saturating_add(1);
    }

    // -------------------------------------------------------------------------
    // Rollup functions - aggregate fine buckets into coarse buckets
    // -------------------------------------------------------------------------

    fn rollup_all(&mut self, now_ms: i64) {
        self.rollup_ms_to_hundred_ms(now_ms);
        self.rollup_hundred_ms_to_seconds(now_ms);
        self.rollup_seconds_to_minutes(now_ms);
        self.rollup_minutes_to_hours(now_ms);
        self.rollup_hours_to_days(now_ms);
    }

    fn rollup_ms_to_hundred_ms(&mut self, now_ms: i64) {
        let current = now_ms / 100;
        if self.last_hundred_ms == 0 {
            self.last_hundred_ms = current;
            return;
        }

        while self.last_hundred_ms < current {
            let start_ms = self.last_hundred_ms * 100;
            let sum: u32 = (0..100)
                .map(|i| {
                    let idx = (start_ms + i) as usize % MS_BUCKETS;
                    let val = self.ms[idx] as u32;
                    self.ms[idx] = 0;
                    val
                })
                .sum();

            if sum > 0 {
                let idx = (self.last_hundred_ms % HUNDRED_MS_BUCKETS as i64) as usize;
                self.hundred_ms[idx] =
                    self.hundred_ms[idx].saturating_add(sum.min(u16::MAX as u32) as u16);
            }
            self.last_hundred_ms += 1;
        }
    }

    fn rollup_hundred_ms_to_seconds(&mut self, now_ms: i64) {
        let current = now_ms / MS_PER_SECOND;
        if self.last_second == 0 {
            self.last_second = current;
            return;
        }

        while self.last_second < current {
            let start = self.last_second * 10;
            let sum: u32 = (0..10)
                .map(|i| {
                    let idx = (start + i) as usize % HUNDRED_MS_BUCKETS;
                    let val = self.hundred_ms[idx] as u32;
                    self.hundred_ms[idx] = 0;
                    val
                })
                .sum();

            if sum > 0 {
                let idx = (self.last_second % SECONDS_BUCKETS as i64) as usize;
                self.seconds[idx] =
                    self.seconds[idx].saturating_add(sum.min(u16::MAX as u32) as u16);
            }
            self.last_second += 1;
        }
    }

    fn rollup_seconds_to_minutes(&mut self, now_ms: i64) {
        let current = now_ms / MS_PER_MINUTE;
        if self.last_minute == 0 {
            self.last_minute = current;
            return;
        }

        while self.last_minute < current {
            let start = self.last_minute * SECS_PER_MINUTE;
            let sum: u32 = (0..SECS_PER_MINUTE)
                .map(|i| {
                    let idx = (start + i) as usize % SECONDS_BUCKETS;
                    let val = self.seconds[idx] as u32;
                    self.seconds[idx] = 0;
                    val
                })
                .sum();

            if sum > 0 {
                let idx = (self.last_minute % MINUTES_BUCKETS as i64) as usize;
                self.minutes[idx] =
                    self.minutes[idx].saturating_add(sum.min(u16::MAX as u32) as u16);
            }
            self.last_minute += 1;
        }
    }

    fn rollup_minutes_to_hours(&mut self, now_ms: i64) {
        let current = now_ms / MS_PER_HOUR;
        if self.last_hour == 0 {
            self.last_hour = current;
            return;
        }

        while self.last_hour < current {
            let start = self.last_hour * MINS_PER_HOUR;
            let sum: u32 = (0..MINS_PER_HOUR)
                .map(|i| {
                    let idx = (start + i) as usize % MINUTES_BUCKETS;
                    let val = self.minutes[idx] as u32;
                    self.minutes[idx] = 0;
                    val
                })
                .sum();

            if sum > 0 {
                let idx = (self.last_hour % HOURS_BUCKETS as i64) as usize;
                self.hours[idx] = self.hours[idx].saturating_add(sum);
            }
            self.last_hour += 1;
        }
    }

    fn rollup_hours_to_days(&mut self, now_ms: i64) {
        let current = now_ms / MS_PER_DAY;
        if self.last_day == 0 {
            self.last_day = current;
            return;
        }

        while self.last_day < current {
            let start = self.last_day * HOURS_PER_DAY;
            let sum: u32 = (0..HOURS_PER_DAY)
                .map(|i| {
                    let idx = (start + i) as usize % HOURS_BUCKETS;
                    let val = self.hours[idx] as u32;
                    self.hours[idx] = 0;
                    val
                })
                .sum();

            if sum > 0 {
                let idx = (self.last_day % DAYS_BUCKETS as i64) as usize;
                self.days[idx] = self.days[idx].saturating_add(sum);
            }
            self.last_day += 1;
        }
    }

    // -------------------------------------------------------------------------
    // Window checking - count requests in time range
    // -------------------------------------------------------------------------

    fn check_window(
        &self,
        window_ms: i64,
        limit: u32,
        now_ms: i64,
        is_calendar_day: bool,
    ) -> (bool, u32, u32) {
        let total = match window_ms {
            x if x <= 120 => self.sum_ms_range(now_ms, window_ms),
            x if x <= 9000 => self.sum_hundred_ms_range(now_ms, window_ms),
            x if x <= MS_PER_HOUR => self.sum_seconds_range(now_ms, window_ms),
            x if x <= 2 * MS_PER_HOUR => self.sum_minutes_range(now_ms, window_ms, is_calendar_day),
            x if x <= 2 * MS_PER_DAY => self.sum_hours_range(now_ms, window_ms, is_calendar_day),
            _ => self.sum_days_range(now_ms, window_ms, is_calendar_day),
        };

        let allowed = total < limit;
        let remaining = limit.saturating_sub(total);
        let retry_after = if allowed { 0 } else { window_ms as u32 };

        (allowed, remaining, retry_after)
    }

    #[inline]
    fn sum_ms_range(&self, now_ms: i64, window_ms: i64) -> u32 {
        (0..window_ms.min(MS_BUCKETS as i64))
            .map(|i| self.ms[((now_ms - i).rem_euclid(MS_BUCKETS as i64)) as usize] as u32)
            .sum()
    }

    #[inline]
    fn sum_hundred_ms_range(&self, now_ms: i64, window_ms: i64) -> u32 {
        let current_hm = now_ms / 100;
        let window_hm = (window_ms + 99) / 100;
        let lookback = window_hm.min(HUNDRED_MS_BUCKETS as i64);

        let full: u32 = (1..=lookback)
            .map(|i| {
                self.hundred_ms[((current_hm - i).rem_euclid(HUNDRED_MS_BUCKETS as i64)) as usize]
                    as u32
            })
            .sum();

        let partial_ms = now_ms % 100;
        let partial: u32 = (0..partial_ms)
            .map(|i| self.ms[((now_ms - i).rem_euclid(MS_BUCKETS as i64)) as usize] as u32)
            .sum();

        full + partial
    }

    #[inline]
    fn sum_seconds_range(&self, now_ms: i64, window_ms: i64) -> u32 {
        let current_sec = now_ms / MS_PER_SECOND;
        let window_secs = (window_ms + 999) / MS_PER_SECOND;
        let lookback = window_secs.min(SECONDS_BUCKETS as i64);

        let full: u32 = (1..=lookback)
            .map(|i| {
                self.seconds[((current_sec - i).rem_euclid(SECONDS_BUCKETS as i64)) as usize] as u32
            })
            .sum();

        let partial_hm_count = (now_ms % MS_PER_SECOND / 100).min(10);
        let partial_hm: u32 = (0..partial_hm_count)
            .map(|i| {
                self.hundred_ms[((now_ms / 100 - i).rem_euclid(HUNDRED_MS_BUCKETS as i64)) as usize]
                    as u32
            })
            .sum();

        let partial_ms_count = now_ms % 100;
        let partial_ms: u32 = (0..partial_ms_count)
            .map(|i| self.ms[((now_ms - i).rem_euclid(MS_BUCKETS as i64)) as usize] as u32)
            .sum();

        full + partial_hm + partial_ms
    }

    fn sum_minutes_range(&self, now_ms: i64, window_ms: i64, is_calendar_day: bool) -> u32 {
        let current_sec = now_ms / MS_PER_SECOND;
        let window_start_sec = if is_calendar_day {
            (current_sec / SECS_PER_DAY) * SECS_PER_DAY
        } else {
            current_sec.saturating_sub(window_ms / MS_PER_SECOND)
        };

        let window_minutes =
            ((current_sec - window_start_sec + 59) / SECS_PER_MINUTE).min(MINUTES_BUCKETS as i64);

        let minutes: u32 = (0..window_minutes)
            .map(|i| {
                self.minutes[(((window_start_sec / SECS_PER_MINUTE) + i)
                    .rem_euclid(MINUTES_BUCKETS as i64)) as usize] as u32
            })
            .sum();

        let partial_sec = current_sec % SECS_PER_MINUTE;
        let seconds: u32 = (0..partial_sec)
            .map(|i| {
                self.seconds[((current_sec - i).rem_euclid(SECONDS_BUCKETS as i64)) as usize] as u32
            })
            .sum();

        let partial_hm = (now_ms % MS_PER_SECOND / 100).min(10);
        let hm: u32 = (0..partial_hm)
            .map(|i| {
                self.hundred_ms[((now_ms / 100 - i).rem_euclid(HUNDRED_MS_BUCKETS as i64)) as usize]
                    as u32
            })
            .sum();

        let partial_ms = now_ms % 100;
        let ms: u32 = (0..partial_ms)
            .map(|i| self.ms[((now_ms - i).rem_euclid(MS_BUCKETS as i64)) as usize] as u32)
            .sum();

        minutes + seconds + hm + ms
    }

    fn sum_hours_range(&self, now_ms: i64, window_ms: i64, is_calendar_day: bool) -> u32 {
        let now_sec = now_ms / MS_PER_SECOND;
        let window_start_sec = if is_calendar_day {
            (now_sec / SECS_PER_DAY) * SECS_PER_DAY
        } else {
            now_sec.saturating_sub(window_ms / MS_PER_SECOND)
        };

        let start_hour = (window_start_sec + SECS_PER_HOUR - 1) / SECS_PER_HOUR;
        let end_hour = now_sec / SECS_PER_HOUR;

        // Partial minutes at start
        let partial_start = if window_start_sec % SECS_PER_HOUR != 0 {
            let next_hour_boundary = start_hour * SECS_PER_HOUR;
            let partial_min = (next_hour_boundary - window_start_sec + 59) / SECS_PER_MINUTE;
            (0..partial_min)
                .map(|i| {
                    self.minutes[((window_start_sec / SECS_PER_MINUTE + i)
                        .rem_euclid(MINUTES_BUCKETS as i64))
                        as usize] as u32
                })
                .sum()
        } else {
            0
        };

        // Complete hours
        let full: u32 = (start_hour..end_hour)
            .map(|h| self.hours[(h.rem_euclid(HOURS_BUCKETS as i64)) as usize] as u32)
            .sum();

        // Complete minutes in current hour
        let current_hour_start_sec = end_hour * SECS_PER_HOUR;
        let current_minute = now_sec / SECS_PER_MINUTE;
        let hour_start_minute = current_hour_start_sec / SECS_PER_MINUTE;
        let partial_end_minutes: u32 = (hour_start_minute..current_minute)
            .map(|m| self.minutes[(m.rem_euclid(MINUTES_BUCKETS as i64)) as usize] as u32)
            .sum();

        // Complete seconds in current minute
        let current_minute_start_sec = current_minute * SECS_PER_MINUTE;
        let complete_seconds: u32 = (current_minute_start_sec..now_sec)
            .map(|s| self.seconds[(s.rem_euclid(SECONDS_BUCKETS as i64)) as usize] as u32)
            .sum();

        // Complete hundred_ms in current second
        let current_second_start_hm = now_sec * 10;
        let current_hm = now_ms / 100;
        let complete_hm: u32 = (current_second_start_hm..current_hm)
            .map(|h| self.hundred_ms[(h.rem_euclid(HUNDRED_MS_BUCKETS as i64)) as usize] as u32)
            .sum();

        // Current ms bucket
        let current_hm_start_ms = current_hm * 100;
        let current_ms: u32 = (current_hm_start_ms..=now_ms)
            .map(|m| self.ms[(m.rem_euclid(MS_BUCKETS as i64)) as usize] as u32)
            .sum();

        partial_start + full + partial_end_minutes + complete_seconds + complete_hm + current_ms
    }

    fn sum_days_range(&self, now_ms: i64, window_ms: i64, is_calendar_day: bool) -> u32 {
        let now_sec = now_ms / MS_PER_SECOND;

        if is_calendar_day {
            let day_start_sec = (now_sec / SECS_PER_DAY) * SECS_PER_DAY;
            let current_hour = now_sec / SECS_PER_HOUR;
            let day_start_hour = day_start_sec / SECS_PER_HOUR;

            // Complete hours from start of day
            let complete_hours: u32 = (day_start_hour..current_hour)
                .map(|h| self.hours[(h.rem_euclid(HOURS_BUCKETS as i64)) as usize] as u32)
                .sum();

            // Complete minutes in current hour
            let current_hour_start_sec = current_hour * SECS_PER_HOUR;
            let current_minute = now_sec / SECS_PER_MINUTE;
            let hour_start_minute = current_hour_start_sec / SECS_PER_MINUTE;
            let complete_minutes: u32 = (hour_start_minute..current_minute)
                .map(|m| self.minutes[(m.rem_euclid(MINUTES_BUCKETS as i64)) as usize] as u32)
                .sum();

            // Complete seconds in current minute
            let current_minute_start_sec = current_minute * SECS_PER_MINUTE;
            let complete_seconds: u32 = (current_minute_start_sec..now_sec)
                .map(|s| self.seconds[(s.rem_euclid(SECONDS_BUCKETS as i64)) as usize] as u32)
                .sum();

            // Complete hundred_ms in current second
            let current_second_start_hm = now_sec * 10;
            let current_hm = now_ms / 100;
            let complete_hm: u32 = (current_second_start_hm..current_hm)
                .map(|h| self.hundred_ms[(h.rem_euclid(HUNDRED_MS_BUCKETS as i64)) as usize] as u32)
                .sum();

            // Current ms bucket
            let current_hm_start_ms = current_hm * 100;
            let current_ms: u32 = (current_hm_start_ms..=now_ms)
                .map(|m| self.ms[(m.rem_euclid(MS_BUCKETS as i64)) as usize] as u32)
                .sum();

            complete_hours + complete_minutes + complete_seconds + complete_hm + current_ms
        } else {
            // Rolling window across multiple days
            let window_start_sec = now_sec.saturating_sub(window_ms / MS_PER_SECOND);
            let start_day = (window_start_sec + SECS_PER_DAY - 1) / SECS_PER_DAY;
            let end_day = now_sec / SECS_PER_DAY;

            let partial_start = if window_start_sec % SECS_PER_DAY != 0 {
                let next_day_boundary = start_day * SECS_PER_DAY;
                let partial_hours =
                    (next_day_boundary - window_start_sec + SECS_PER_HOUR - 1) / SECS_PER_HOUR;
                (0..partial_hours)
                    .map(|i| {
                        self.hours[((window_start_sec / SECS_PER_HOUR + i)
                            .rem_euclid(HOURS_BUCKETS as i64))
                            as usize] as u32
                    })
                    .sum()
            } else {
                0
            };

            let full: u32 = (start_day..end_day)
                .map(|d| self.days[(d.rem_euclid(DAYS_BUCKETS as i64)) as usize] as u32)
                .sum();

            let partial_end = if now_sec % SECS_PER_DAY != 0 {
                let current_day_start = end_day * SECS_PER_DAY;
                let partial_hours = (now_sec - current_day_start) / SECS_PER_HOUR;
                (0..partial_hours)
                    .map(|i| {
                        self.hours[((current_day_start / SECS_PER_HOUR + i)
                            .rem_euclid(HOURS_BUCKETS as i64))
                            as usize] as u32
                    })
                    .sum()
            } else {
                0
            };

            partial_start + full + partial_end
        }
    }
}
