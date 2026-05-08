use common::error::AppError;
use models::AgentSkill;

pub struct ListAgentSkillsQuery {
    pub deployment_id: i64,
    pub agent_id: i64,
}

impl ListAgentSkillsQuery {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Vec<AgentSkill>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            AgentSkill,
            r#"
            SELECT deployment_id, agent_id, slug, name, description, storage_prefix,
                   created_at, updated_at
            FROM agent_skills
            WHERE deployment_id = $1 AND agent_id = $2
            ORDER BY slug
            "#,
            self.deployment_id,
            self.agent_id
        )
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;
        Ok(rows)
    }
}
