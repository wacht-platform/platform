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
}

impl Query for GetSmsPricingQuery {
    type Output = Option<Decimal>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query_scalar!(
            "SELECT price_cents FROM sms_country_pricing WHERE country_code = $1",
            self.country_code.to_uppercase()
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(result)
    }
}
