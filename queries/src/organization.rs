use common::error::AppError;

pub struct GetOrganizationNameQuery {
    organization_id: i64,
}

impl GetOrganizationNameQuery {
    pub fn new(organization_id: i64) -> Self {
        Self { organization_id }
    }

    pub async fn execute_with_db<'a, A>(&self, acquirer: A) -> Result<String, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let row = sqlx::query!(
            "SELECT name FROM organizations WHERE id = $1",
            self.organization_id
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(row.name)
    }
}
