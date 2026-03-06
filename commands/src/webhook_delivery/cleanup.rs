use sqlx::{Executor, Postgres, query};

use crate::Command;
use common::{error::AppError, state::AppState};

#[derive(Debug)]
pub struct CleanupExpiredDeliveriesCommand {
    pub days_old: i32,
}

impl CleanupExpiredDeliveriesCommand {
    async fn execute_with_deps<'e, E>(self, executor: E) -> Result<i64, AppError>
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

    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<i64, AppError>
    where
        A: sqlx::Acquire<'a, Database = Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        self.execute_with_deps(&mut *conn).await
    }
}

impl Command for CleanupExpiredDeliveriesCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
