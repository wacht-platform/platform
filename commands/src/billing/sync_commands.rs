use crate::Command;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use rust_decimal::Decimal;

pub struct CreateBillingSyncRunCommand {
    pub from_event_id: i64,
}

impl Command for CreateBillingSyncRunCommand {
    type Output = i64;

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(state.db_router.writer()).await
    }
}

impl CreateBillingSyncRunCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<i64, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let rec = sqlx::query!(
            "INSERT INTO billing_sync_runs (from_event_id, to_event_id, status)
             VALUES ($1, 0, 'running')
             RETURNING id",
            self.from_event_id
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(rec.id)
    }
}

pub struct CompleteBillingSyncRunCommand {
    pub sync_run_id: i64,
    pub events_processed: i64,
    pub deployments_affected: i32,
}

impl Command for CompleteBillingSyncRunCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(state.db_router.writer()).await
    }
}

impl CompleteBillingSyncRunCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query!(
            "UPDATE billing_sync_runs
             SET completed_at = NOW(),
                 events_processed = $2,
                 deployments_affected = $3,
                 status = 'completed'
             WHERE id = $1",
            self.sync_run_id,
            self.events_processed,
            self.deployments_affected
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}

pub struct UpsertUsageSnapshotCommand {
    pub deployment_id: i64,
    pub billing_account_id: i64,
    pub billing_period: DateTime<Utc>,
    pub metric_name: String,
    pub quantity: i64,
    pub cost_cents: Option<Decimal>,
}

impl Command for UpsertUsageSnapshotCommand {
    type Output = ();

    async fn execute(self, state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "INSERT INTO billing_usage_snapshots
             (deployment_id, billing_account_id, billing_period, metric_name, quantity, cost_cents, min_event_id, max_event_id)
             VALUES ($1, $2, $3, $4, $5, $6, 0, 0)
             ON CONFLICT (deployment_id, billing_period, metric_name)
             DO UPDATE SET
                billing_account_id = EXCLUDED.billing_account_id,
                quantity = billing_usage_snapshots.quantity + $5,
                cost_cents = COALESCE(billing_usage_snapshots.cost_cents, 0) + COALESCE($6, 0),
                updated_at = NOW()",
            self.deployment_id,
            self.billing_account_id,
            self.billing_period,
            self.metric_name,
            self.quantity,
            self.cost_cents
        )
        .execute(state.db_router.writer())
        .await?;

        Ok(())
    }
}
