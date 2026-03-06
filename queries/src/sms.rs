use crate::Query;
use common::error::AppError;
use common::state::AppState;
use rust_decimal::Decimal;

pub struct GetSmsPricingQuery {
    pub country_code: String,
}

impl GetSmsPricingQuery {
    pub fn new(country_code: String) -> Self {
        Self { country_code }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Option<Decimal>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = sqlx::query_scalar!(
            "SELECT price_cents FROM sms_country_pricing WHERE country_code = $1",
            self.country_code.to_uppercase()
        )
        .fetch_optional(&mut *conn)
        .await?;

        Ok(result)
    }
}

impl Query for GetSmsPricingQuery {
    type Output = Option<Decimal>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}
