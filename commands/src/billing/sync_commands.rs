use common::error::AppError;

pub struct CreateBillingSyncRunCommand {
    pub from_event_id: i64,
}

impl CreateBillingSyncRunCommand {
    pub fn new(from_event_id: i64) -> Self {
        Self { from_event_id }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<i64, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rec = sqlx::query!(
            "INSERT INTO billing_sync_runs (from_event_id, to_event_id, status)
             VALUES ($1, 0, 'running')
             RETURNING id",
            self.from_event_id
        )
        .fetch_one(executor)
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
    pub fn new(sync_run_id: i64, events_processed: i64, deployments_affected: i32) -> Self {
        Self {
            sync_run_id,
            events_processed,
            deployments_affected,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        .execute(executor)
        .await?;

        Ok(())
    }
}
