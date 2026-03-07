use chrono::{DateTime, Utc};
use common::error::AppError;
use rust_decimal::Decimal;

pub struct CreateBillingSyncRunCommand {
    pub from_event_id: i64,
}

impl CreateBillingSyncRunCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<i64, AppError>
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

impl CompleteBillingSyncRunCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<(), AppError>
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
