use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::{Executor, Postgres, Transaction, query};

use crate::Command;
use common::{
    capabilities::{HasDbRouter, HasRedis},
    error::AppError,
    state::AppState,
};

#[derive(Debug)]
pub struct GetActiveDeliveryCommand {
    pub delivery_id: i64,
}

impl GetActiveDeliveryCommand {
    async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Option<ActiveDeliveryInfo>, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let delivery = query!(
            r#"
            SELECT d.id as "id!",
                   d.endpoint_id as "endpoint_id",
                   d.event_name as "event_name!",
                   d.payload as "payload!",
                   d.filter_rules,
                   d.webhook_id as "webhook_id!",
                   d.webhook_timestamp as "webhook_timestamp!",
                   d.signature as "signature",
                   d.attempts as "attempts",
                   d.max_attempts as "max_attempts",
                   d.next_retry_at,
                   d.created_at as "created_at",
                   e.url as "url!",
                   e.headers,
                   e.timeout_seconds,
                   e.max_retries,
                   e.app_slug as "app_slug!",
                   e.rate_limit_config,
                   a.signing_secret as "signing_secret!"
            FROM active_webhook_deliveries d
            JOIN webhook_endpoints e ON d.endpoint_id = e.id
            JOIN webhook_apps a ON (e.deployment_id = a.deployment_id AND e.app_slug = a.app_slug)
            WHERE d.id = $1
            "#,
            self.delivery_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(delivery.map(|d| ActiveDeliveryInfo {
            id: d.id,
            endpoint_id: d.endpoint_id.unwrap_or(0),
            event_name: d.event_name,
            payload: Some(d.payload),
            filter_rules: d.filter_rules,
            webhook_id: d.webhook_id,
            webhook_timestamp: d.webhook_timestamp,
            signature: d.signature,
            attempts: d.attempts.unwrap_or(0),
            max_attempts: d.max_attempts.unwrap_or(5),
            next_retry_at: d.next_retry_at,
            created_at: d.created_at.unwrap_or_else(Utc::now),
            url: d.url,
            headers: d.headers,
            timeout_seconds: d.timeout_seconds.unwrap_or(30),
            max_retries: d.max_retries.unwrap_or(5),
            app_slug: d.app_slug,
            signing_secret: d.signing_secret,
            rate_limit_config: d.rate_limit_config,
        }))
    }

    pub async fn execute_with<C>(self, deps: &C) -> Result<Option<ActiveDeliveryInfo>, AppError>
    where
        C: HasDbRouter + ?Sized,
    {
        self.execute_with_db(deps.writer_pool()).await
    }

    pub async fn execute_in_tx(
        self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<Option<ActiveDeliveryInfo>, AppError> {
        self.execute_with_db(tx.as_mut()).await
    }
}

impl Command for GetActiveDeliveryCommand {
    type Output = Option<ActiveDeliveryInfo>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}

#[derive(Debug)]
pub struct ActiveDeliveryInfo {
    pub id: i64,
    pub endpoint_id: i64,
    pub event_name: String,
    pub payload: Option<Value>,
    pub filter_rules: Option<Value>,
    pub webhook_id: String,
    pub webhook_timestamp: i64,
    pub signature: Option<String>,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub headers: Option<Value>,
    pub timeout_seconds: i32,
    pub max_retries: i32,
    pub app_slug: String,
    pub signing_secret: String,
    pub rate_limit_config: Option<Value>,
}

#[derive(Debug)]
pub struct DeleteActiveDeliveryCommand {
    pub delivery_id: i64,
}

impl DeleteActiveDeliveryCommand {
    async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        query!(
            "DELETE FROM active_webhook_deliveries WHERE id = $1",
            self.delivery_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }

    pub async fn execute_with<C>(self, deps: &C) -> Result<(), AppError>
    where
        C: HasDbRouter + ?Sized,
    {
        self.execute_with_db(deps.writer_pool()).await
    }

    pub async fn execute_in_tx(self, tx: &mut Transaction<'_, Postgres>) -> Result<(), AppError> {
        self.execute_with_db(tx.as_mut()).await
    }
}

impl Command for DeleteActiveDeliveryCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}

#[derive(Debug)]
pub struct UpdateDeliveryAttemptsCommand {
    pub delivery_id: i64,
    pub new_attempts: i32,
    pub next_retry_at: DateTime<Utc>,
}

impl UpdateDeliveryAttemptsCommand {
    async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        query!(
            r#"
            UPDATE active_webhook_deliveries 
            SET attempts = $2, next_retry_at = $3
            WHERE id = $1
            "#,
            self.delivery_id,
            self.new_attempts,
            self.next_retry_at
        )
        .execute(executor)
        .await?;

        Ok(())
    }

    pub async fn execute_with<C>(self, deps: &C) -> Result<(), AppError>
    where
        C: HasDbRouter + ?Sized,
    {
        self.execute_with_db(deps.writer_pool()).await
    }

    pub async fn execute_in_tx(self, tx: &mut Transaction<'_, Postgres>) -> Result<(), AppError> {
        self.execute_with_db(tx.as_mut()).await
    }
}

impl Command for UpdateDeliveryAttemptsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}

#[derive(Debug)]
pub struct GetFailedDeliveryDetailsCommand {
    pub delivery_id: i64,
}

impl Command for GetFailedDeliveryDetailsCommand {
    type Output = Option<String>;

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        Ok(None)
    }
}

#[derive(Debug)]
pub struct DeactivateEndpointCommand {
    pub endpoint_id: i64,
}

impl DeactivateEndpointCommand {
    async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        query!(
            r#"
            UPDATE webhook_endpoints
            SET is_active = false,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.endpoint_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }

    pub async fn execute_with<C>(self, deps: &C) -> Result<(), AppError>
    where
        C: HasDbRouter + ?Sized,
    {
        self.execute_with_db(deps.writer_pool()).await
    }

    pub async fn execute_in_tx(self, tx: &mut Transaction<'_, Postgres>) -> Result<(), AppError> {
        self.execute_with_db(tx.as_mut()).await
    }
}

impl Command for DeactivateEndpointCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}

#[derive(Debug)]
pub struct CheckEndpointFailuresCommand {
    pub endpoint_id: i64,
}

impl CheckEndpointFailuresCommand {
    pub async fn execute_with<C>(self, deps: &C) -> Result<EndpointFailureInfo, AppError>
    where
        C: HasRedis + ?Sized,
    {
        use redis::AsyncCommands;

        let mut redis_conn = deps
            .redis_client()
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Redis connection failed: {}", e)))?;

        let failure_key = format!("webhook:endpoint:failures:{}", self.endpoint_id);
        let failure_count: i64 = redis_conn.get(&failure_key).await.unwrap_or(0);

        Ok(EndpointFailureInfo {
            failure_count,
            should_deactivate: failure_count >= 10,
        })
    }
}

impl Command for CheckEndpointFailuresCommand {
    type Output = EndpointFailureInfo;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}

#[derive(Debug)]
pub struct EndpointFailureInfo {
    pub failure_count: i64,
    pub should_deactivate: bool,
}

#[derive(Debug)]
pub struct IncrementEndpointFailuresCommand {
    pub endpoint_id: i64,
}

impl IncrementEndpointFailuresCommand {
    pub async fn execute_with<C>(self, deps: &C) -> Result<i64, AppError>
    where
        C: HasRedis + ?Sized,
    {
        use redis::AsyncCommands;

        let mut redis_conn = deps
            .redis_client()
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Redis connection failed: {}", e)))?;

        let failure_key = format!("webhook:endpoint:failures:{}", self.endpoint_id);
        let failure_count: i64 = redis_conn
            .incr(&failure_key, 1)
            .await
            .map_err(|e| AppError::Internal(format!("Redis incr failed: {}", e)))?;

        if failure_count == 1 {
            let _: () = redis_conn
                .expire(&failure_key, 86400)
                .await
                .map_err(|e| AppError::Internal(format!("Redis expire failed: {}", e)))?;
        }

        Ok(failure_count)
    }
}

impl Command for IncrementEndpointFailuresCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}

#[derive(Debug)]
pub struct ClearEndpointFailuresCommand {
    pub endpoint_id: i64,
}

impl ClearEndpointFailuresCommand {
    pub async fn execute_with<C>(self, deps: &C) -> Result<(), AppError>
    where
        C: HasRedis + ?Sized,
    {
        use redis::AsyncCommands;

        let mut redis_conn = deps
            .redis_client()
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Redis connection failed: {}", e)))?;

        let failure_key = format!("webhook:endpoint:failures:{}", self.endpoint_id);
        let _: () = redis_conn
            .del(&failure_key)
            .await
            .map_err(|e| AppError::Internal(format!("Redis del failed: {}", e)))?;

        Ok(())
    }
}

impl Command for ClearEndpointFailuresCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}

pub fn calculate_next_retry(attempts: i32) -> DateTime<Utc> {
    let delay = match attempts {
        1 => Duration::seconds(30),
        2 => Duration::minutes(1),
        3 => Duration::minutes(5),
        4 => Duration::minutes(15),
        _ => Duration::hours(6),
    };

    Utc::now() + delay
}

#[derive(Debug)]
pub struct CleanupExpiredDeliveriesCommand {
    pub days_old: i32,
}

impl CleanupExpiredDeliveriesCommand {
    async fn execute_with_db<'e, E>(self, executor: E) -> Result<i64, AppError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let result = query!(
            r#"
            DELETE FROM active_webhook_deliveries
            WHERE created_at < NOW() - INTERVAL '1 day' * $1
            AND attempts >= max_attempts
            "#,
            self.days_old as f64
        )
        .execute(executor)
        .await?;

        Ok(result.rows_affected() as i64)
    }

    pub async fn execute_with<C>(self, deps: &C) -> Result<i64, AppError>
    where
        C: HasDbRouter + ?Sized,
    {
        self.execute_with_db(deps.writer_pool()).await
    }

    pub async fn execute_in_tx(self, tx: &mut Transaction<'_, Postgres>) -> Result<i64, AppError> {
        self.execute_with_db(tx.as_mut()).await
    }
}

impl Command for CleanupExpiredDeliveriesCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}
