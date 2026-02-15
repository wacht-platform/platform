use chrono::{DateTime, Utc};
use common::{error::AppError, state::AppState};

use crate::Command;

pub struct SyncBillingMetricsCommand {
    pub deployment_id: i64,
    pub billing_account_id: i64,
    pub billing_period: DateTime<Utc>,
    pub metrics: Vec<(String, i64)>,
    pub redis_prefix: String,
}

impl Command for SyncBillingMetricsCommand {
    type Output = Vec<(String, i64)>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await?;

        for (metric_name, quantity) in &self.metrics {
            sqlx::query(
                "INSERT INTO billing_usage_snapshots 
                 (deployment_id, billing_account_id, billing_period, metric_name, quantity, cost_cents, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
                 ON CONFLICT (deployment_id, billing_period, metric_name)
                 DO UPDATE SET quantity = $5, updated_at = NOW()"
            )
            .bind(self.deployment_id)
            .bind(self.billing_account_id)
            .bind(self.billing_period)
            .bind(metric_name)
            .bind(*quantity)
            .bind(None::<rust_decimal::Decimal>)
            .execute(&mut *tx)
            .await?;
        }

        let mut redis = app_state
            .redis_client
            .get_multiplexed_async_connection()
            .await?;

        let lua_script = create_redis_update_script(&self.redis_prefix, &self.metrics);
        let script = redis::Script::new(&lua_script);
        let _: String = script.invoke_async(&mut redis).await?;

        tx.commit().await?;

        Ok(self.metrics.clone())
    }
}

fn create_redis_update_script(prefix: &str, metrics: &[(String, i64)]) -> String {
    let mut script = format!("local prefix = '{}'\n", prefix);
    script.push_str("local last_synced_key = prefix .. ':last_synced'\n");

    for (metric_name, quantity) in metrics {
        let redis_key = match metric_name.as_str() {
            "ai_token_input_cost_cents" => "ai_token_input_cost",
            "ai_token_output_cost_cents" => "ai_token_output_cost",
            "sms_cost" => "sms_cost_cents",
            _ => metric_name.as_str(),
        };
        script.push_str(&format!(
            "redis.call('ZADD', last_synced_key, {}, '{}')\n",
            quantity, redis_key
        ));
    }

    script.push_str("redis.call('EXPIRE', last_synced_key, 5184000)\n");
    script.push_str("return 'OK'");
    script
}
