use common::error::AppError;
use common::state::AppState;

use super::Query;

pub struct GetOrganizationNameQuery {
    organization_id: i64,
}

impl GetOrganizationNameQuery {
    pub fn new(organization_id: i64) -> Self {
        Self { organization_id }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<String, AppError>
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

impl Query for GetOrganizationNameQuery {
    type Output = String;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
