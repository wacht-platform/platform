use chrono::{DateTime, Duration as ChronoDuration, Utc};
use common::{HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, error::AppError};
use models::{ProjectTaskBoardItem, ProjectTaskBoardItemAssignment};

use crate::project_task_board::{
    enqueue_assignment_execution_event_with_deps, enqueue_board_item_to_coordinator_with_deps,
};

pub struct ReconcileStaleBoardItemsCommand {
    pub stale_threshold: ChronoDuration,
    pub max_items_per_tick: i64,
}

#[derive(Debug, Default, Clone)]
pub struct ReconcileStaleBoardItemsSummary {
    pub rerouted_to_assignment: u64,
    pub rerouted_to_coordinator: u64,
    pub skipped: u64,
}

impl ReconcileStaleBoardItemsSummary {
    pub fn total_rerouted(&self) -> u64 {
        self.rerouted_to_assignment + self.rerouted_to_coordinator
    }
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
        D: HasDbRouter + HasIdProvider + HasNatsJetStreamProvider + ?Sized,
    {
        let stale_before = Utc::now() - self.stale_threshold;
        let mut summary = ReconcileStaleBoardItemsSummary::default();

        for _ in 0..self.max_items_per_tick {
            let picked = pick_and_touch_stale_board_item(deps, stale_before).await?;
            let Some(picked) = picked else {
                break;
            };

            match picked {
                PickedItem::WithAssignment(item, assignment) => {
                    match enqueue_assignment_execution_event_with_deps(deps, &assignment).await {
                        Ok(()) => summary.rerouted_to_assignment += 1,
                        Err(err) => {
                            tracing::warn!(
                                board_item_id = item.id,
                                assignment_id = assignment.id,
                                %err,
                                "Reconciler failed to enqueue assignment_execution; will retry next tick",
                            );
                            summary.skipped += 1;
                        }
                    }
                }
                PickedItem::NoAssignment(item) => {
                    match enqueue_board_item_to_coordinator_with_deps(
                        deps,
                        &item,
                        Some("Reconciler: board item has not progressed".to_string()),
                        None,
                    )
                    .await
                    {
                        Ok(()) => summary.rerouted_to_coordinator += 1,
                        Err(err) => {
                            tracing::warn!(
                                board_item_id = item.id,
                                %err,
                                "Reconciler failed to enqueue task_routing; will retry next tick",
                            );
                            summary.skipped += 1;
                        }
                    }
                }
            }
        }

        Ok(summary)
    }
}

enum PickedItem {
    WithAssignment(ProjectTaskBoardItem, ProjectTaskBoardItemAssignment),
    NoAssignment(ProjectTaskBoardItem),
}

async fn pick_and_touch_stale_board_item<D>(
    deps: &D,
    stale_before: DateTime<Utc>,
) -> Result<Option<PickedItem>, AppError>
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
            id, board_id, task_key, title, description, status, priority,
            assigned_thread_id, metadata, completed_at, archived_at,
            created_at, updated_at
        "#,
        stale_before,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(board_item) = board_item else {
        tx.commit().await?;
        return Ok(None);
    };

    let assignment = sqlx::query_as!(
        ProjectTaskBoardItemAssignment,
        r#"
        SELECT
            id, board_item_id, thread_id, assignment_role, assignment_order, status,
            instructions, metadata, result_status, result_summary,
            result_payload, claimed_at, started_at, completed_at, rejected_at, created_at,
            updated_at
        FROM project_task_board_item_assignments
        WHERE board_item_id = $1
          AND status IN ('pending', 'available', 'claimed', 'in_progress')
        ORDER BY assignment_order ASC, id ASC
        LIMIT 1
        "#,
        board_item.id,
    )
    .fetch_optional(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Some(match assignment {
        Some(assignment) => PickedItem::WithAssignment(board_item, assignment),
        None => PickedItem::NoAssignment(board_item),
    }))
}
