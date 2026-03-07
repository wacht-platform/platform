use common::error::AppError;

pub struct GetOrganizationNameQuery {
    organization_id: i64,
}

impl GetOrganizationNameQuery {
    pub fn new(organization_id: i64) -> Self {
        Self { organization_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<String, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            "SELECT name FROM organizations WHERE id = $1",
            self.organization_id
        )
        .fetch_one(executor)
        .await?;

        Ok(row.name)
    }
}
