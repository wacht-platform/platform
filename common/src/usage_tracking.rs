use chrono::{DateTime, Datelike, NaiveDate, Utc};
use sqlx::{PgPool, Postgres, Transaction};
use crate::snowflake::SnowflakeGenerator;

/// Usage tracking service for billing
/// Tracks all billable metrics (MAU, orgs, workspaces, emails, SMS, webhooks, AI tokens, storage)
pub struct UsageTrackingService {
    pool: PgPool,
    snowflake: SnowflakeGenerator,
}

impl UsageTrackingService {
    pub fn new(pool: PgPool, snowflake: SnowflakeGenerator) -> Self {
        Self { pool, snowflake }
    }

    /// Get current billing period start date (first day of current month)
    fn get_billing_period_start() -> NaiveDate {
        let now = Utc::now();
        NaiveDate::from_ymd_opt(now.year(), now.month(), 1)
            .expect("Failed to create billing period start date")
    }

    /// Get billing period end date (last day of current month)
    fn get_billing_period_end() -> NaiveDate {
        let now = Utc::now();
        let next_month = if now.month() == 12 {
            NaiveDate::from_ymd_opt(now.year() + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(now.year(), now.month() + 1, 1)
        }.expect("Failed to create next month date");

        next_month.pred_opt().expect("Failed to get last day of month")
    }

    /// Track MAU (Monthly Active User) - upsert to avoid duplicates
    pub async fn track_mau(
        &self,
        deployment_id: i64,
        user_id: i64,
    ) -> Result<(), sqlx::Error> {
        let billing_period_start = Self::get_billing_period_start();
        let id = self.snowflake.generate();

        sqlx::query!(
            r#"
            INSERT INTO monthly_active_users (id, deployment_id, user_id, billing_period_start, last_active_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (deployment_id, user_id, billing_period_start)
            DO UPDATE SET last_active_at = NOW()
            "#,
            id,
            deployment_id,
            user_id,
            billing_period_start
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Increment usage for a metric (organizations, workspaces, emails, webhooks, etc.)
    pub async fn increment_usage(
        &self,
        deployment_id: i64,
        metric_name: &str,
        quantity: i64,
    ) -> Result<(), sqlx::Error> {
        self.increment_usage_in_tx(&self.pool, deployment_id, metric_name, quantity).await
    }

    /// Increment usage within a transaction
    pub async fn increment_usage_in_tx<'a>(
        &self,
        executor: impl sqlx::Executor<'a, Database = Postgres>,
        deployment_id: i64,
        metric_name: &str,
        quantity: i64,
    ) -> Result<(), sqlx::Error> {
        let billing_period_start = Self::get_billing_period_start();
        let billing_period_end = Self::get_billing_period_end();
        let id = self.snowflake.generate();

        sqlx::query!(
            r#"
            INSERT INTO usage_tracking (
                id, deployment_id, billing_period_start, billing_period_end,
                metric_name, quantity, synced_to_chargebee
            )
            VALUES ($1, $2, $3, $4, $5, $6, false)
            ON CONFLICT (deployment_id, billing_period_start, metric_name)
            DO UPDATE SET
                quantity = usage_tracking.quantity + $6,
                updated_at = NOW()
            "#,
            id,
            deployment_id,
            billing_period_start,
            billing_period_end,
            metric_name,
            quantity
        )
        .execute(executor)
        .await?;

        Ok(())
    }

    /// Track SMS with country-specific pricing
    pub async fn track_sms(
        &self,
        deployment_id: i64,
        phone_number: &str,
        country_code: &str,
        cost_cents: rust_decimal::Decimal,
        provider_message_id: Option<String>,
    ) -> Result<(), sqlx::Error> {
        let billing_period_start = Self::get_billing_period_start();
        let id = self.snowflake.generate();

        // Hash phone number for privacy (SHA256)
        let phone_hash = sha256::digest(phone_number);

        sqlx::query!(
            r#"
            INSERT INTO sms_usage_log (
                id, deployment_id, billing_period_start, phone_number_hash,
                country_code, cost_cents, provider_message_id, status, sent_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, 'sent', NOW())
            "#,
            id,
            deployment_id,
            billing_period_start,
            phone_hash,
            country_code,
            cost_cents,
            provider_message_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Get SMS cost by phone number prefix
    pub async fn get_sms_cost(&self, phone_number: &str) -> Result<rust_decimal::Decimal, sqlx::Error> {
        let cost = sqlx::query_scalar!(
            r#"SELECT get_sms_cost_by_phone($1) as "cost!""#,
            phone_number
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(cost)
    }

    /// Track AI tokens (input and output separately)
    pub async fn track_ai_tokens(
        &self,
        deployment_id: i64,
        input_tokens: i64,
        output_tokens: i64,
    ) -> Result<(), sqlx::Error> {
        // Track input tokens
        if input_tokens > 0 {
            self.increment_usage(deployment_id, "llm_tokens_input", input_tokens).await?;
        }

        // Track output tokens
        if output_tokens > 0 {
            self.increment_usage(deployment_id, "llm_tokens_output", output_tokens).await?;
        }

        Ok(())
    }

    /// Get current MAU count for a deployment
    pub async fn get_current_mau(&self, deployment_id: i64) -> Result<i64, sqlx::Error> {
        let billing_period_start = Self::get_billing_period_start();

        let count = sqlx::query_scalar!(
            r#"
            SELECT COUNT(DISTINCT user_id) as "count!"
            FROM monthly_active_users
            WHERE deployment_id = $1 AND billing_period_start = $2
            "#,
            deployment_id,
            billing_period_start
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(count)
    }

    /// Get current usage for a specific metric
    pub async fn get_current_usage(
        &self,
        deployment_id: i64,
        metric_name: &str,
    ) -> Result<i64, sqlx::Error> {
        let billing_period_start = Self::get_billing_period_start();

        let quantity = sqlx::query_scalar!(
            r#"
            SELECT COALESCE(quantity, 0) as "quantity!"
            FROM usage_tracking
            WHERE deployment_id = $1
              AND billing_period_start = $2
              AND metric_name = $3
            "#,
            deployment_id,
            billing_period_start,
            metric_name
        )
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(0);

        Ok(quantity)
    }

    /// Get total SMS costs for current billing period
    pub async fn get_current_sms_costs(&self, deployment_id: i64) -> Result<rust_decimal::Decimal, sqlx::Error> {
        let billing_period_start = Self::get_billing_period_start();

        let total_cost = sqlx::query_scalar!(
            r#"
            SELECT COALESCE(SUM(cost_cents), 0) as "total!"
            FROM sms_usage_log
            WHERE deployment_id = $1 AND billing_period_start = $2
            "#,
            deployment_id,
            billing_period_start
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(total_cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_billing_period_calculation() {
        let start = UsageTrackingService::get_billing_period_start();
        let end = UsageTrackingService::get_billing_period_end();

        assert_eq!(start.day(), 1);
        assert!(end.day() >= 28); // Last day of month
        assert!(end.day() <= 31);
    }
}
