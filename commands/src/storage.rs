use common::{error::AppError, state::AppState};

use crate::Command;

pub struct MarkStorageAsCleanCommand {
    pub deployment_id: i64,
}

impl Command for MarkStorageAsCleanCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl MarkStorageAsCleanCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query!(
            "UPDATE deployment_storage_usage SET is_dirty = false WHERE deployment_id = $1",
            self.deployment_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
