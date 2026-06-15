//! Worker-side metric definitions for the event-driven architecture.
//!
//! These metrics are registered against the global OTel meter set up in
//! `common::init_telemetry`. They flow to the same OTLP endpoint as traces and
//! logs (Grafana / wherever `OTEL_EXPORTER_OTLP_ENDPOINT` points).
//!
//! Each metric is a `LazyLock` so the first access lazily registers it.
//! Update them inline at the relevant call sites (dispatcher, recovery cron,
//! agent execution, schedule dispatcher).
//!
//! Naming convention: `{component}_{thing}_{unit}` where unit follows OTel
//! semantic conventions (`_total` for counters, `_seconds` for time, no suffix
//! for gauges).

use opentelemetry::KeyValue;
use opentelemetry::global;
use opentelemetry::metrics::{Counter, Histogram, Meter};
use std::sync::LazyLock;

static METER: LazyLock<Meter> = LazyLock::new(|| global::meter("wacht-worker"));

/// Counter — events written to event_log, labeled by `event_type`.
pub static EVENT_LOG_INSERTED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("event_log_inserted_total")
        .with_description("Events inserted into event_log (before dedup collisions)")
        .build()
});

/// Counter — events that hit the unique idempotency_key dedup. Should be > 0
/// in normal operation if concurrent state mutations exist.
pub static EVENT_LOG_DEDUPED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("event_log_deduped_total")
        .with_description("Insert-into-event_log calls that hit ON CONFLICT DO NOTHING")
        .build()
});

/// Histogram — dispatcher publish latency in seconds. Labeled by `event_type`.
pub static EVENT_LOG_PUBLISH_LATENCY: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    METER
        .f64_histogram("event_log_publish_latency_seconds")
        .with_description("Time from claim to NATS publish ack")
        .build()
});

/// Counter — dispatcher publish failures, labeled by `event_type`.
pub static EVENT_LOG_PUBLISH_FAILED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("event_log_publish_failed_total")
        .with_description("Failed publish attempts; reset to pending if retries remain")
        .build()
});

/// Counter — events that exhausted publish retries and went to dead-letter.
pub static EVENT_LOG_DEAD_LETTERED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("event_log_dead_lettered_total")
        .with_description("Events with publish_status='failed' (retries exhausted)")
        .build()
});

/// Counter — work_lease rows reclaimed by the recovery cron after expiring.
/// Non-zero is normal (retries, slow workers); a sustained spike means workers
/// are crashing or stuck.
pub static WORK_LEASE_EXPIRED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("work_lease_expired_total")
        .with_description("Leases reclaimed by recovery cron after expiry")
        .build()
});

/// Counter — work_lease claim attempts that lost the race (someone else holds
/// the lease). Expected to be > 0 under load (NATS redelivery, dispatcher
/// double-publish under transient errors).
pub static WORK_LEASE_LOST_RACE: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("work_lease_lost_race_total")
        .with_description("INSERT INTO work_lease that returned no row (lease already held)")
        .build()
});

/// Histogram — assignment-execution duration in seconds. Labeled by
/// `assignment_role` (executor / reviewer / specialist_reviewer / approver / observer).
pub static ASSIGNMENT_DURATION: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    METER
        .f64_histogram("assignment_duration_seconds")
        .with_description("Wall-clock time from claim to completion")
        .build()
});

/// Histogram — schedule fire latency: how late the dispatcher fired vs
/// `next_run_at`. p99 should be < 60s under normal load.
pub static SCHEDULE_FIRE_LATENCY: LazyLock<Histogram<f64>> = LazyLock::new(|| {
    METER
        .f64_histogram("schedule_fire_latency_seconds")
        .with_description("now() - next_run_at at the moment the schedule actually fires")
        .build()
});

/// Counter — assignments observed in `claimed`/`in_progress` for longer than
/// the configured staleness threshold with no active work_lease. Sustained
/// non-zero values indicate orphaned assignments (worker died after lease
/// release but before assignment status update, or upstream bug).
pub static STUCK_ASSIGNMENT_DETECTED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("stuck_assignment_detected_total")
        .with_description(
            "Assignments stuck in claimed/in_progress past threshold with no active lease",
        )
        .build()
});

/// Counter — stuck assignments marked blocked and reconciled to the coordinator.
pub static STUCK_ASSIGNMENT_RECOVERED: LazyLock<Counter<u64>> = LazyLock::new(|| {
    METER
        .u64_counter("stuck_assignment_recovered_total")
        .with_description("Stuck assignments marked blocked and reconciled to the coordinator")
        .build()
});

/// Helper: build a single-pair KeyValue slice for labels.
pub fn label(key: &'static str, value: impl Into<String>) -> [KeyValue; 1] {
    [KeyValue::new(key, value.into())]
}

/// Helper: build a two-pair KeyValue slice.
pub fn labels2(
    key1: &'static str,
    value1: impl Into<String>,
    key2: &'static str,
    value2: impl Into<String>,
) -> [KeyValue; 2] {
    [
        KeyValue::new(key1, value1.into()),
        KeyValue::new(key2, value2.into()),
    ]
}
