use crate::Command;
use common::error::AppError;
use common::state::AppState;
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
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        redis_client: &redis::Client,
    ) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        self.execute_with_deps(&mut conn, redis_client).await
    }

    pub(crate) async fn execute_with_deps(
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

impl Command for ClearDeploymentCacheCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer(), &app_state.redis_client)
            .await
    }
}
