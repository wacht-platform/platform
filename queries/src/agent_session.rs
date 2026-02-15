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
}

impl Query for GetAgentSessionQuery {
    type Output = Option<AgentSession>;

    async fn execute(&self, app_state: &AppState) -> StdResult<Self::Output, AppError> {
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
        .fetch_optional(&app_state.db_pool)
        .await?;

        Ok(result)
    }
}
