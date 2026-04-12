use chrono::{DateTime, Utc};
use common::{HasDbRouter, HasRedisProvider, error::AppError};

pub struct SyncBillingMetricsCommand {
    pub deployment_id: i64,
    pub billing_account_id: i64,
    pub billing_period: DateTime<Utc>,
    pub metrics: Vec<(String, i64)>,
    pub redis_prefix: String,
}

impl SyncBillingMetricsCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<(String, i64)>, AppError>
    where
        D: HasDbRouter + HasRedisProvider,
    {
        let writer = deps.db_router().writer();
        let redis_client = deps.redis_provider();

        for (metric_name, quantity) in &self.metrics {
            sqlx::query!(
                "INSERT INTO billing_usage_snapshots
                 (deployment_id, billing_account_id, billing_period, metric_name, quantity, cost_cents, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
                 ON CONFLICT (deployment_id, billing_period, metric_name)
                 DO UPDATE SET quantity = $5, updated_at = NOW()"
                ,
                self.deployment_id,
                self.billing_account_id,
                self.billing_period,
                metric_name,
                *quantity,
                Option::<rust_decimal::Decimal>::None
            )
            .execute(writer)
            .await?;
        }

        let mut redis = redis_client.get_multiplexed_async_connection().await?;

        let lua_script = create_redis_update_script(&self.redis_prefix, &self.metrics);
        let script = redis::Script::new(&lua_script);
        let _: String = script.invoke_async(&mut redis).await?;

        Ok(self.metrics.clone())
    }
}

fn create_redis_update_script(prefix: &str, metrics: &[(String, i64)]) -> String {
    let mut script = format!("local prefix = '{}'\n", prefix);
    script.push_str("local last_synced_key = prefix .. ':last_synced'\n");

    for (metric_name, quantity) in metrics {
        let redis_key = match metric_name.as_str() {
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
