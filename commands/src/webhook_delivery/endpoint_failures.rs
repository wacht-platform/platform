use chrono::{DateTime, Duration, Utc};

use crate::Command;
use common::{
    capabilities::HasRedis,
    error::AppError,
    state::AppState,
};

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
