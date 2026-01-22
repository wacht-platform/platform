use common::{error::AppError, state::AppState};

use crate::Command;

pub struct MarkStorageAsCleanCommand {
    pub deployment_id: i64,
}

impl Command for MarkStorageAsCleanCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "UPDATE deployment_storage_usage SET is_dirty = false WHERE deployment_id = $1",
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}
