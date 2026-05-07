//! Detects assignments stuck in `claimed` or `in_progress` past a staleness
//! threshold with no active `work_lease`. Emits `STUCK_ASSIGNMENT_DETECTED`
//! per stale row so ops can alert. Recovery (re-publish or auto-reject) is
//! deliberately deferred — investigation is what's needed first.

use std::time::Duration;

use anyhow::Result;
use commands::event_log::list_stuck_assignments;
use common::state::AppState;
use tracing::{error, warn};

use crate::metrics::STUCK_ASSIGNMENT_DETECTED;

const SWEEP_INTERVAL: Duration = Duration::from_secs(120);
const STALE_AFTER_SECS: i64 = 30 * 60;
const BATCH_LIMIT: i64 = 200;

pub async fn run(app_state: AppState) -> Result<()> {
    let mut tick = tokio::time::interval(SWEEP_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tick.tick().await;
        if let Err(e) = sweep_once(&app_state).await {
            error!(error = %e, "stuck-assignment sweep failed");
        }
    }
}

async fn sweep_once(app_state: &AppState) -> Result<()> {
    let rows =
        list_stuck_assignments(app_state.db_router.writer(), STALE_AFTER_SECS, BATCH_LIMIT).await?;

    if rows.is_empty() {
        return Ok(());
    }

    STUCK_ASSIGNMENT_DETECTED.add(rows.len() as u64, &[]);
    warn!(count = rows.len(), "stuck assignments detected");

    Ok(())
}
