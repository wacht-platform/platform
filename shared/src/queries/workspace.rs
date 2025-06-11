use crate::{
    error::AppError,
    state::AppState,
};

use super::Query;

pub struct GetWorkspaceNameQuery {
    workspace_id: i64,
}

impl GetWorkspaceNameQuery {
    pub fn new(workspace_id: i64) -> Self {
        Self { workspace_id }
    }
}

impl Query for GetWorkspaceNameQuery {
    type Output = String;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            "SELECT name FROM workspaces WHERE id = $1",
            self.workspace_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(row.name)
    }
}
