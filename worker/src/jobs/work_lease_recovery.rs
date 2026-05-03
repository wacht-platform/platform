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
use tracing::{error, info};

use crate::metrics::{EVENT_LOG_DEAD_LETTERED, WORK_LEASE_EXPIRED};

const RECOVERY_INTERVAL: Duration = Duration::from_secs(60);

/// Spawn the recovery loop. Runs forever.
pub async fn run(app_state: AppState) -> Result<()> {
    info!(
        "work-lease recovery cron starting (interval = {:?})",
        RECOVERY_INTERVAL
    );
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

    // Reclaim expired leases that still have retry budget.
    match reclaim_expired_leases(pool, MAX_LEASE_ATTEMPTS).await {
        Ok(reclaimed) => {
            if !reclaimed.is_empty() {
                let n = reclaimed.len() as u64;
                WORK_LEASE_EXPIRED.add(n, &[]);
                info!(count = n, "reclaimed expired leases for retry");
            }
        }
        Err(e) => {
            error!(error = %e, "reclaim_expired_leases failed");
        }
    }

    // Dead-letter leases that exhausted their retry budget.
    match mark_exhausted_leases_failed(pool, MAX_LEASE_ATTEMPTS).await {
        Ok(n) if n > 0 => {
            EVENT_LOG_DEAD_LETTERED.add(n, &[]);
            info!(count = n, "dead-lettered events that exhausted lease retries");
        }
        Ok(_) => {}
        Err(e) => {
            error!(error = %e, "mark_exhausted_leases_failed failed");
        }
    }

    Ok(())
}
