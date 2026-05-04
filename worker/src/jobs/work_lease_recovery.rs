//! Work-lease recovery cron.
//!
//! Runs every 60s. Two responsibilities:
//!   1. Reclaim leases past their `expires_at` (worker crashed / heartbeat
//!      stopped) — reset `event_log.publish_status='pending'` so the
//!      dispatcher republishes for retry.
//!   2. Mark leases that exhausted their retry budget as `failed` (dead
//!      letter), so a stuck task doesn't loop forever.

use std::time::Duration;

use anyhow::Result;
use commands::event_log::{
    MAX_LEASE_ATTEMPTS, mark_exhausted_leases_failed, reclaim_expired_leases,
};
use common::state::AppState;
use tracing::error;

use crate::metrics::{EVENT_LOG_DEAD_LETTERED, WORK_LEASE_EXPIRED};

const RECOVERY_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn the recovery loop. Runs forever.
pub async fn run(app_state: AppState) -> Result<()> {
    let mut tick = tokio::time::interval(RECOVERY_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        if let Err(e) = sweep_once(&app_state).await {
            error!(error = %e, "work-lease recovery sweep failed");
        }
    }
}

async fn sweep_once(app_state: &AppState) -> Result<()> {
    let pool = app_state.db_router.writer();

    match reclaim_expired_leases(pool, MAX_LEASE_ATTEMPTS).await {
        Ok(reclaimed) => {
            if !reclaimed.is_empty() {
                WORK_LEASE_EXPIRED.add(reclaimed.len() as u64, &[]);
            }
        }
        Err(e) => {
            error!(error = %e, "reclaim_expired_leases failed");
        }
    }

    match mark_exhausted_leases_failed(pool, MAX_LEASE_ATTEMPTS).await {
        Ok(n) if n > 0 => {
            EVENT_LOG_DEAD_LETTERED.add(n, &[]);
        }
        Ok(_) => {}
        Err(e) => {
            error!(error = %e, "mark_exhausted_leases_failed failed");
        }
    }

    Ok(())
}
