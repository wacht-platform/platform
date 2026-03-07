use common::error::AppError;

pub struct GetWorkspaceNameQuery {
    workspace_id: i64,
}

impl GetWorkspaceNameQuery {
    pub fn new(workspace_id: i64) -> Self {
        Self { workspace_id }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<String, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            "SELECT name FROM workspaces WHERE id = $1",
            self.workspace_id
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(row.name)
    }
}
