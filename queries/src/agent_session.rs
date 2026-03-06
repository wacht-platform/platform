use crate::prelude::*;
use models::AgentSession;

pub struct GetAgentSessionQuery {
    pub session_id: i64,
    pub deployment_id: i64,
}

impl GetAgentSessionQuery {
    pub fn new(session_id: i64, deployment_id: i64) -> Self {
        Self {
            session_id,
            deployment_id,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> StdResult<Option<AgentSession>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = sqlx::query_as::<_, AgentSession>(
            r#"
            SELECT id, session_id, deployment_id, identifier, context_group,
                   agent_ids, expires_at
            FROM agent_sessions
            WHERE session_id = $1
              AND deployment_id = $2
              AND (expires_at IS NULL OR expires_at > NOW())
            LIMIT 1
            "#,
        )
        .bind(self.session_id)
        .bind(self.deployment_id)
        .fetch_optional(&mut *conn)
        .await?;

        Ok(result)
    }
}

impl Query for GetAgentSessionQuery {
    type Output = Option<AgentSession>;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}
