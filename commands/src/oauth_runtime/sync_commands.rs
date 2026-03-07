use chrono::Utc;
use common::{HasDbRouter, HasRedis, error::AppError};
use redis::AsyncCommands;

const OAUTH_GRANT_LAST_USED_DIRTY_KEY: &str = "oauth:grant:last_used:dirty";

pub struct EnqueueOAuthGrantLastUsed {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub grant_id: i64,
}

impl EnqueueOAuthGrantLastUsed {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasRedis,
    {
        let mut redis_conn = deps
            .redis_client()
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to connect redis: {}", e)))?;

        let member = format!(
            "{}:{}:{}",
            self.deployment_id, self.oauth_client_id, self.grant_id
        );
        let score = Utc::now().timestamp_millis() as f64;
        let _: () = redis_conn
            .zadd(OAUTH_GRANT_LAST_USED_DIRTY_KEY, member, score)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to enqueue grant usage: {}", e)))?;
        let _: bool = redis_conn
            .expire(OAUTH_GRANT_LAST_USED_DIRTY_KEY, 604800)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to set dirty-key expiry: {}", e)))?;

        Ok(())
    }
}

pub struct SyncOAuthGrantLastUsedBatch {
    pub batch_size: usize,
}

impl SyncOAuthGrantLastUsedBatch {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<usize, AppError>
    where
        D: HasRedis + HasDbRouter,
    {
        let batch_size = self.batch_size.max(1);

        let mut redis_conn = deps
            .redis_client()
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to connect redis: {}", e)))?;

        let popped: Vec<(String, f64)> = redis::cmd("ZPOPMIN")
            .arg(OAUTH_GRANT_LAST_USED_DIRTY_KEY)
            .arg(batch_size)
            .query_async(&mut redis_conn)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to pop dirty grants: {}", e)))?;

        if popped.is_empty() {
            return Ok(0);
        }

        let mut deployment_ids = Vec::with_capacity(popped.len());
        let mut client_ids = Vec::with_capacity(popped.len());
        let mut grant_ids = Vec::with_capacity(popped.len());
        let mut used_ats = Vec::with_capacity(popped.len());

        for (member, score) in popped {
            let mut parts = member.split(':');
            let (Some(deployment_part), Some(client_part), Some(grant_part), None) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };

            let (Ok(deployment_id), Ok(oauth_client_id), Ok(grant_id)) = (
                deployment_part.parse::<i64>(),
                client_part.parse::<i64>(),
                grant_part.parse::<i64>(),
            ) else {
                continue;
            };

            let Some(used_at) =
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(score as i64)
            else {
                continue;
            };
            deployment_ids.push(deployment_id);
            client_ids.push(oauth_client_id);
            grant_ids.push(grant_id);
            used_ats.push(used_at);
        }

        if deployment_ids.is_empty() {
            return Ok(0);
        }

        let synced = grant_ids.len();
        let writer_pool = deps.writer_pool();

        sqlx::query(
            r#"
            WITH input AS (
                SELECT
                    UNNEST($1::bigint[]) AS deployment_id,
                    UNNEST($2::bigint[]) AS oauth_client_id,
                    UNNEST($3::bigint[]) AS grant_id,
                    UNNEST($4::timestamptz[]) AS used_at
            )
            UPDATE oauth_client_grants g
            SET
                last_used_at = GREATEST(COALESCE(g.last_used_at, input.used_at), input.used_at),
                updated_at = NOW()
            FROM input
            WHERE g.deployment_id = input.deployment_id
              AND g.oauth_client_id = input.oauth_client_id
              AND g.id = input.grant_id
            "#,
        )
        .bind(&deployment_ids)
        .bind(&client_ids)
        .bind(&grant_ids)
        .bind(&used_ats)
        .execute(writer_pool)
        .await?;

        Ok(synced)
    }
}
