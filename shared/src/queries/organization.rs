use crate::{
    error::AppError,
    state::AppState,
};

use super::Query;

pub struct GetOrganizationNameQuery {
    organization_id: i64,
}

impl GetOrganizationNameQuery {
    pub fn new(organization_id: i64) -> Self {
        Self { organization_id }
    }
}

impl Query for GetOrganizationNameQuery {
    type Output = String;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let row = sqlx::query!(
            "SELECT name FROM organizations WHERE id = $1",
            self.organization_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(row.name)
    }
}
