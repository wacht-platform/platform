use sqlx::{Executor, Postgres, Transaction, query};

use crate::Command;
use common::{
    capabilities::HasDbRouter,
    error::AppError,
    state::AppState,
};

#[derive(Debug)]
pub struct CleanupExpiredDeliveriesCommand {
    pub days_old: i32,
}

impl CleanupExpiredDeliveriesCommand {
    async fn execute_with_db<'e, E>(self, executor: E) -> Result<i64, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = query!(
            r#"
            DELETE FROM active_webhook_deliveries
            WHERE created_at < NOW() - INTERVAL '1 day' * $1
            AND attempts >= max_attempts
            "#,
            self.days_old as f64
        )
        .execute(executor)
        .await?;

        Ok(result.rows_affected() as i64)
    }

    pub async fn execute_with<C>(self, deps: &C) -> Result<i64, AppError>
    where
        C: HasDbRouter + ?Sized,
    {
        self.execute_with_db(deps.writer_pool()).await
    }

    pub async fn execute_in_tx(self, tx: &mut Transaction<'_, Postgres>) -> Result<i64, AppError> {
        self.execute_with_db(tx.as_mut()).await
    }
}

impl Command for CleanupExpiredDeliveriesCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}
