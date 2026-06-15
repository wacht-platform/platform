//! Detects assignments stuck in `claimed`/`in_progress` with no active lease,
//! and recovers the ones that can never progress on their own: triggering event
//! already terminal, no live lease, board item still open, not awaiting a human.
//! Those are marked `blocked` and reconciled back to the coordinator.

use std::time::Duration;

use anyhow::Result;
use commands::UpdateProjectTaskBoardItemAssignmentStateCommand;
use commands::event_log::{list_recoverable_stuck_assignments, list_stuck_assignments};
use common::state::AppState;
use models::project_task_board::{assignment_result_status, assignment_status};
use tracing::{error, info, warn};

use crate::metrics::{STUCK_ASSIGNMENT_DETECTED, STUCK_ASSIGNMENT_RECOVERED};

const SWEEP_INTERVAL: Duration = Duration::from_secs(120);
const STALE_AFTER_SECS: i64 = 30 * 60;
const DETECT_BATCH_LIMIT: i64 = 200;
const RECOVER_BATCH_LIMIT: i64 = 50;

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
    let detected = list_stuck_assignments(
        app_state.db_router.writer(),
        STALE_AFTER_SECS,
        DETECT_BATCH_LIMIT,
    )
    .await?;
    if !detected.is_empty() {
        STUCK_ASSIGNMENT_DETECTED.add(detected.len() as u64, &[]);
        warn!(count = detected.len(), "stuck assignments detected");
    }

    let recoverable = list_recoverable_stuck_assignments(
        app_state.db_router.writer(),
        STALE_AFTER_SECS,
        RECOVER_BATCH_LIMIT,
    )
    .await?;
    if recoverable.is_empty() {
        return Ok(());
    }

    let deps = common::deps::from_app(app_state).db().nats().id();
    let mut recovered = 0u64;
    for row in &recoverable {
        if let Err(e) = recover_one(&deps, row.assignment_id).await {
            warn!(
                assignment_id = row.assignment_id,
                board_item_id = row.board_item_id,
                error = %e,
                "failed to recover stuck assignment",
            );
            continue;
        }
        recovered += 1;
    }

    if recovered > 0 {
        STUCK_ASSIGNMENT_RECOVERED.add(recovered, &[]);
        info!(recovered, "recovered stuck assignments to coordinator");
    }

    Ok(())
}

/// Mark a terminally-stuck assignment `blocked`; `execute_with_deps` reconciles,
/// routing the board item back to the coordinator (or auto-completing if owned).
async fn recover_one<D>(deps: &D, assignment_id: i64) -> Result<()>
where
    D: common::HasDbRouter
        + common::HasIdProvider
        + common::HasNatsJetStreamProvider
        + common::HasNatsProvider,
{
    UpdateProjectTaskBoardItemAssignmentStateCommand::new(
        assignment_id,
        assignment_status::BLOCKED.to_string(),
    )
    .with_result(
        Some(assignment_result_status::FAILED.to_string()),
        Some(
            "Recovered by the stuck-assignment backstop: execution terminated without \
             finalizing this assignment. Returned to the coordinator to re-plan."
                .to_string(),
        ),
        None,
    )
    .with_note("Stuck assignment auto-recovered; returned to coordinator.".to_string())
    .execute_with_deps(deps)
    .await?;
    Ok(())
}
