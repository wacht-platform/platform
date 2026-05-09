use common::error::AppError;
use sqlx::Postgres;

pub struct UpsertAgentSkillCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub storage_prefix: String,
}

impl UpsertAgentSkillCommand {
    pub async fn execute<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"
            INSERT INTO agent_skills
                (deployment_id, agent_id, slug, name, description, storage_prefix)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (deployment_id, agent_id, slug) DO UPDATE
                SET name = EXCLUDED.name,
                    description = EXCLUDED.description,
                    storage_prefix = EXCLUDED.storage_prefix,
                    updated_at = NOW()
            "#,
            self.deployment_id,
            self.agent_id,
            self.slug,
            self.name,
            self.description,
            self.storage_prefix,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}

pub struct DeleteAgentSkillCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub slug: String,
}

impl DeleteAgentSkillCommand {
    pub async fn execute<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = Postgres>,
    {
        sqlx::query!(
            r#"DELETE FROM agent_skills WHERE deployment_id = $1 AND agent_id = $2 AND slug = $3"#,
            self.deployment_id,
            self.agent_id,
            self.slug,
        )
        .execute(executor)
        .await?;
        Ok(())
    }
}
