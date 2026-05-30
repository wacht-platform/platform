use chrono::{DateTime, Duration as ChronoDuration, Utc};
use common::{
    HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, HasNatsProvider, error::AppError,
};
use models::ProjectTaskBoardItem;

use crate::project_task_board::ReconcileProjectTaskBoardItemCommand;

pub struct ReconcileStaleBoardItemsCommand {
    pub stale_threshold: ChronoDuration,
    pub max_items_per_tick: i64,
}

#[derive(Debug, Default, Clone)]
pub struct ReconcileStaleBoardItemsSummary {
    pub reconciled: u64,
    pub skipped: u64,
}

impl ReconcileStaleBoardItemsCommand {
    pub fn new(stale_threshold: ChronoDuration, max_items_per_tick: i64) -> Self {
        Self {
            stale_threshold,
            max_items_per_tick,
        }
    }

    pub async fn execute_with_deps<D>(
        &self,
        deps: &D,
    ) -> Result<ReconcileStaleBoardItemsSummary, AppError>
    where
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + HasNatsProvider + ?Sized,
    {
        let stale_before = Utc::now() - self.stale_threshold;
        let mut summary = ReconcileStaleBoardItemsSummary::default();

        for _ in 0..self.max_items_per_tick {
            let Some(board_item) = pick_and_touch_stale_board_item(deps, stale_before).await?
            else {
                break;
            };

            // Re-run the one canonical reconcile — the single source of truth. It dispatches
            // available/pending assignments to their assigned threads, auto-completes agent-owned
            // (delegated) items, and routes ONLY coordinator-owned items to the coordinator (a
            // delegated item always has exclusive_owner_agent_id set, so it can never reach the
            // coordinator branch). Execution liveness/crash recovery is owned by the lease layer
            // (work_lease_recovery), so we never re-dispatch in-flight work here.
            match ReconcileProjectTaskBoardItemCommand::new(board_item.id)
                .with_note("Reconciler: stale board item re-reconciled".to_string())
                .execute_with_deps(deps)
                .await
            {
                Ok(()) => summary.reconciled += 1,
                Err(err) => {
                    tracing::warn!(
                        board_item_id = board_item.id,
                        %err,
                        "Reconciler failed to reconcile stale board item; will retry next tick",
                    );
                    summary.skipped += 1;
                }
            }
        }

        Ok(summary)
    }
}

async fn pick_and_touch_stale_board_item<D>(
    deps: &D,
    stale_before: DateTime<Utc>,
) -> Result<Option<ProjectTaskBoardItem>, AppError>
where
    D: HasDbRouter + ?Sized,
{
    let mut tx = deps.writer_pool().begin().await?;

    let board_item = sqlx::query_as!(
        ProjectTaskBoardItem,
        r#"
        UPDATE project_task_board_items
        SET updated_at = NOW()
        WHERE id = (
            SELECT id
            FROM project_task_board_items
            WHERE status IN ('pending', 'in_progress')
              AND archived_at IS NULL
              AND updated_at < $1
            ORDER BY updated_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
        )
        RETURNING
            id, board_id, task_key, title, description, status,
            assigned_thread_id, metadata, completed_at, archived_at,
            created_at, updated_at, state_version,
            schedule_id, scheduled_for, fired_at, pending_question, pending_approval, mounts, exclusive_owner_agent_id,
            deliverables
        "#,
        stale_before,
    )
    .fetch_optional(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(board_item)
}
