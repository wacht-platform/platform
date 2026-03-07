use common::error::AppError;

pub struct GetWorkspaceNameQuery {
    workspace_id: i64,
}

impl GetWorkspaceNameQuery {
    pub fn new(workspace_id: i64) -> Self {
        Self { workspace_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<String, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            "SELECT name FROM workspaces WHERE id = $1",
            self.workspace_id
        )
        .fetch_one(executor)
        .await?;

        Ok(row.name)
    }
}
