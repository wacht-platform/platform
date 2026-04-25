use common::error::AppError;
use models::ComposioSettingsRow;

pub struct GetActiveComposioSlugsForActorQuery {
    deployment_id: i64,
    actor_id: i64,
    candidate_slugs: Vec<String>,
}

impl GetActiveComposioSlugsForActorQuery {
    pub fn new(deployment_id: i64, actor_id: i64, candidate_slugs: Vec<String>) -> Self {
        Self {
            deployment_id,
            actor_id,
            candidate_slugs,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<String>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if self.candidate_slugs.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query!(
            r#"
            SELECT slug
            FROM actor_external_connections
            WHERE deployment_id = $1
              AND actor_id = $2
              AND provider = 'composio'
              AND status = 'active'
              AND slug = ANY($3)
            "#,
            self.deployment_id,
            self.actor_id,
            &self.candidate_slugs,
        )
        .fetch_all(executor)
        .await?;
        Ok(rows.into_iter().map(|r| r.slug).collect())
    }
}

pub struct GetComposioSettingsQuery {
    deployment_id: i64,
}

impl GetComposioSettingsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ComposioSettingsRow>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT
                composio_enabled AS "enabled!: bool",
                composio_use_platform_key AS "use_platform_key!: bool",
                composio_api_key AS "api_key?: String",
                composio_enabled_apps AS "enabled_apps!: serde_json::Value"
            FROM deployment_ai_settings
            WHERE deployment_id = $1
            "#,
            self.deployment_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| ComposioSettingsRow {
            enabled: r.enabled,
            use_platform_key: r.use_platform_key,
            api_key: r.api_key,
            enabled_apps: r.enabled_apps,
        }))
    }
}
