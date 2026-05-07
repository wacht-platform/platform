//! Outbox dispatcher loop.
//!
//! Reads pending rows from `event_log`, publishes them to NATS, marks them
//! published. Driven by:
//!   1. NATS wake on `agent.outbox.wake` after each event_log insert.
//!   2. A schedule-aware sleep that fires when the earliest retried event
//!      becomes due (for events with `next_publish_at` in the future).
//!   3. A long paranoid-sweep timer as defense-in-depth — recovers any
//!      wake silently lost (NATS hiccup, broker drop, dispatcher restart).
//!      If it routinely finds rows, that's a wake-loss bug worth
//!      investigating, not routine work.
//!
//! Multiple dispatcher instances coexist via `FOR UPDATE SKIP LOCKED`.
//! Crashed-mid-publish recovery via `publishing_started_at + 60s` grace
//! window inside `claim_pending_events`.

use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use commands::event_log::{
    self, MAX_PUBLISH_ATTEMPTS, claim_pending_events, mark_event_failed, mark_event_published,
    next_pending_publish_at, schedule_event_retry,
};
use common::state::AppState;
use futures::StreamExt;
use tokio::time::Instant;
use tracing::{error, warn};

use crate::metrics::{
    EVENT_LOG_DEAD_LETTERED, EVENT_LOG_PUBLISH_FAILED, EVENT_LOG_PUBLISH_LATENCY, label,
};

pub const WAKE_SUBJECT: &str = "agent.outbox.wake";

const BATCH_SIZE: i64 = 100;
const PARANOID_SWEEP: Duration = Duration::from_secs(120);

pub async fn run(app_state: AppState) -> Result<()> {
    let mut wake_sub = app_state
        .nats_client
        .subscribe(WAKE_SUBJECT.to_string())
        .await?;

    drain(&app_state).await;

    loop {
        let next_retry = next_pending_publish_at(app_state.db_router.writer())
            .await
            .ok()
            .flatten();

        tokio::select! {
            biased;
            _ = wake_sub.next() => {
                drain(&app_state).await;
            }
            _ = wait_for_retry(next_retry) => {
                drain(&app_state).await;
            }
            _ = tokio::time::sleep(PARANOID_SWEEP) => {
                paranoid_sweep(&app_state).await;
            }
        }
    }
}

/// Drain everything currently claimable.
async fn drain(app_state: &AppState) {
    loop {
        let claimed = match claim_pending_events(app_state.db_router.writer(), BATCH_SIZE).await {
            Ok(v) => v,
            Err(e) => {
                error!(error = %e, "claim_pending_events failed");
                return;
            }
        };
        if claimed.is_empty() {
            break;
        }
        for event in claimed {
            publish_one(app_state, event).await;
        }
    }
}

/// Sleep until the next future-scheduled event, or forever if none.
async fn wait_for_retry(target: Option<DateTime<Utc>>) {
    match target {
        Some(t) => {
            let dur = (t - Utc::now()).to_std().unwrap_or_default();
            tokio::time::sleep(dur).await;
        }
        None => std::future::pending::<()>().await,
    }
}

/// Defense-in-depth: if the wake path silently drops a message, this
/// catches stranded rows. Hitting this should be rare; loud-log any hit so
/// it's investigated.
async fn paranoid_sweep(app_state: &AppState) {
    let claimed = match claim_pending_events(app_state.db_router.writer(), BATCH_SIZE).await {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "paranoid sweep claim failed");
            return;
        }
    };
    if claimed.is_empty() {
        return;
    }
    warn!(
        count = claimed.len(),
        "paranoid sweep found pending events; wake path likely lost messages"
    );
    for event in claimed {
        publish_one(app_state, event).await;
    }
    drain(app_state).await;
}

async fn publish_one(app_state: &AppState, event: event_log::ClaimedEvent) {
    let started = Instant::now();
    let envelope = serde_json::json!({
        "task_id": event.id.to_string(),
        "task_type": "agent.event_log_work",
        "payload": event.payload,
    });
    let body = envelope.to_string();
    let result = app_state
        .nats_client
        .publish(event.publish_subject.clone(), body.into())
        .await;
    let elapsed = started.elapsed().as_secs_f64();

    EVENT_LOG_PUBLISH_LATENCY.record(elapsed, &label("event_type", event.event_type.clone()));

    match result {
        Ok(_) => {
            if let Err(e) = mark_event_published(app_state.db_router.writer(), event.id).await {
                error!(event_id = event.id, error = %e, "failed to mark event published");
            }
        }
        Err(e) => {
            EVENT_LOG_PUBLISH_FAILED.add(1, &label("event_type", event.event_type.clone()));
            warn!(
                event_id = event.id,
                attempts = event.publish_attempts,
                error = %e,
                "publish failed"
            );
            if event.publish_attempts >= MAX_PUBLISH_ATTEMPTS {
                EVENT_LOG_DEAD_LETTERED.add(1, &label("event_type", event.event_type));
                if let Err(e2) = mark_event_failed(
                    app_state.db_router.writer(),
                    event.id,
                    &format!("dead-lettered: {e}"),
                )
                .await
                {
                    error!(event_id = event.id, error = %e2, "failed to mark event failed");
                }
            } else if let Err(e2) = schedule_event_retry(
                app_state.db_router.writer(),
                event.id,
                event.publish_attempts,
                &e.to_string(),
            )
            .await
            {
                error!(event_id = event.id, error = %e2, "failed to schedule retry");
            }
        }
    }
}
