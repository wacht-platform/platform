use common::error::AppError;
use rust_decimal::Decimal;

pub struct GetSmsPricingQuery {
    pub country_code: String,
}

impl GetSmsPricingQuery {
    pub fn new(country_code: String) -> Self {
        Self { country_code }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<Decimal>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query_scalar!(
            "SELECT price_cents FROM sms_country_pricing WHERE country_code = $1",
            self.country_code.to_uppercase()
        )
        .fetch_optional(executor)
        .await?;

        Ok(result)
    }
}
