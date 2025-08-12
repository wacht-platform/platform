use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sqlx::query;

use crate::{
    error::AppError,
    state::AppState,
};

use super::Command;

#[derive(Debug)]
pub struct GetActiveDeliveryCommand {
    pub delivery_id: i64,
}

impl Command for GetActiveDeliveryCommand {
    type Output = Option<ActiveDeliveryInfo>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let delivery = query!(
            r#"
            SELECT d.id as "id!", 
                   d.endpoint_id as "endpoint_id",
                   d.event_name as "event_name!", 
                   d.payload_s3_key as "payload_s3_key!",
                   d.attempts as "attempts",
                   d.max_attempts as "max_attempts",
                   d.next_retry_at,
                   d.created_at as "created_at",
                   e.url as "url!",
                   e.headers,
                   e.timeout_seconds,
                   e.max_retries,
                   e.ip_allowlist,
                   a.id as "app_id!",
                   a.name as "app_name!",
                   a.signing_secret as "signing_secret!"
            FROM active_webhook_deliveries d
            JOIN webhook_endpoints e ON d.endpoint_id = e.id
            JOIN webhook_apps a ON e.app_id = a.id
            WHERE d.id = $1
            "#,
            self.delivery_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(delivery.map(|d| ActiveDeliveryInfo {
            id: d.id,
            endpoint_id: d.endpoint_id.unwrap_or(0),
            event_name: d.event_name,
            payload_s3_key: d.payload_s3_key,
            attempts: d.attempts.unwrap_or(0),
            max_attempts: d.max_attempts.unwrap_or(5),
            next_retry_at: d.next_retry_at,
            created_at: d.created_at.unwrap_or_else(|| Utc::now()),
            url: d.url,
            headers: d.headers,
            timeout_seconds: d.timeout_seconds.unwrap_or(30),
            max_retries: d.max_retries.unwrap_or(5),
            ip_allowlist: d.ip_allowlist,
            app_id: d.app_id,
            app_name: d.app_name,
            signing_secret: d.signing_secret,
        }))
    }
}

#[derive(Debug)]
pub struct ActiveDeliveryInfo {
    pub id: i64,
    pub endpoint_id: i64,
    pub event_name: String,
    pub payload_s3_key: String,
    pub attempts: i32,
    pub max_attempts: i32,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub headers: Option<Value>,
    pub timeout_seconds: i32,
    pub max_retries: i32,
    pub ip_allowlist: Option<Value>,
    pub app_id: i64,
    pub app_name: String,
    pub signing_secret: String,
}

#[derive(Debug)]
pub struct DeleteActiveDeliveryCommand {
    pub delivery_id: i64,
}

impl Command for DeleteActiveDeliveryCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        query!(
            "DELETE FROM active_webhook_deliveries WHERE id = $1",
            self.delivery_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct UpdateDeliveryAttemptsCommand {
    pub delivery_id: i64,
    pub new_attempts: i32,
    pub next_retry_at: DateTime<Utc>,
}

impl Command for UpdateDeliveryAttemptsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct GetFailedDeliveryDetailsCommand {
    pub delivery_id: i64,
}

impl Command for GetFailedDeliveryDetailsCommand {
    type Output = Option<String>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let delivery = query!(
            "SELECT payload_s3_key FROM active_webhook_deliveries WHERE id = $1",
            self.delivery_id
        )
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(delivery.map(|d| d.payload_s3_key))
    }
}

#[derive(Debug)]
pub struct DeactivateEndpointCommand {
    pub endpoint_id: i64,
}

impl Command for DeactivateEndpointCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        query!(
            r#"
            UPDATE webhook_endpoints
            SET is_active = false,
                updated_at = NOW()
            WHERE id = $1
            "#,
            self.endpoint_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct CheckEndpointFailuresCommand {
    pub endpoint_id: i64,
}

impl Command for CheckEndpointFailuresCommand {
    type Output = EndpointFailureInfo;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        use redis::AsyncCommands;
        
        let mut redis_conn = app_state.redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Redis connection failed: {}", e)))?;

        let failure_key = format!("webhook:endpoint:failures:{}", self.endpoint_id);
        
        // Get current failure count
        let failure_count: i64 = redis_conn
            .get(&failure_key)
            .await
            .unwrap_or(0);

        Ok(EndpointFailureInfo {
            failure_count,
            should_deactivate: failure_count >= 10,
        })
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

impl Command for IncrementEndpointFailuresCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        use redis::AsyncCommands;
        
        let mut redis_conn = app_state.redis_client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Redis connection failed: {}", e)))?;

        let failure_key = format!("webhook:endpoint:failures:{}", self.endpoint_id);
        
        // Increment failure counter
        let failure_count: i64 = redis_conn
            .incr(&failure_key, 1)
            .await
            .map_err(|e| AppError::Internal(format!("Redis incr failed: {}", e)))?;
        
        // Set TTL only on first failure
        if failure_count == 1 {
            let _: () = redis_conn
                .expire(&failure_key, 86400) // 24 hours
                .await
                .map_err(|e| AppError::Internal(format!("Redis expire failed: {}", e)))?;
        }

        Ok(failure_count)
    }
}

#[derive(Debug)]
pub struct ClearEndpointFailuresCommand {
    pub endpoint_id: i64,
}

impl Command for ClearEndpointFailuresCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        use redis::AsyncCommands;
        
        let mut redis_conn = app_state.redis_client
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

pub fn calculate_next_retry(attempts: i32) -> DateTime<Utc> {
    let delay = match attempts {
        1 => Duration::seconds(30),
        2 => Duration::minutes(1),
        3 => Duration::minutes(5),
        4 => Duration::minutes(15),
        _ => Duration::hours(1),
    };
    
    Utc::now() + delay
}

#[derive(Debug)]
pub struct CleanupExpiredDeliveriesCommand {
    pub days_old: i32,
}

impl Command for CleanupExpiredDeliveriesCommand {
    type Output = i64;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Delete deliveries that are too old and have exceeded max attempts
        let result = query!(
            r#"
            DELETE FROM active_webhook_deliveries
            WHERE created_at < NOW() - INTERVAL '1 day' * $1
            AND attempts >= max_attempts
            "#,
            self.days_old as f64
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(result.rows_affected() as i64)
    }
}