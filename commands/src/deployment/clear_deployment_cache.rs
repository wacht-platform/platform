use common::{HasDbRouter, HasRedis, error::AppError};
use redis::AsyncCommands;

pub struct ClearDeploymentCacheCommand {
    pub deployment_id: i64,
}

impl ClearDeploymentCacheCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl ClearDeploymentCacheCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasRedis,
    {
        let mut conn = deps.db_router().writer().acquire().await?;
        let redis_client = deps.redis_client();
        self.execute_with_conn_and_redis(&mut conn, redis_client).await
    }

    pub(crate) async fn execute_with_conn_and_redis(
        self,
        conn: &mut sqlx::PgConnection,
        redis_client: &redis::Client,
    ) -> Result<(), AppError> {
        let deployment_row = sqlx::query!(
            "SELECT backend_host FROM deployments WHERE id = $1",
            self.deployment_id
        )
        .fetch_one(&mut *conn)
        .await?;

        let mut redis_conn = redis_client.get_multiplexed_tokio_connection().await?;

        let cache_key = format!("deployment:{}", deployment_row.backend_host);
        let _: () = redis_conn.del(cache_key).await?;

        Ok(())
    }
}
