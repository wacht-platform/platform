use common::error::AppError;
use models::{Actor, ActorProject, AgentThread};

pub struct GetActorByIdQuery {
    pub actor_id: i64,
    pub deployment_id: i64,
}

pub struct GetActorByExternalKeyQuery {
    pub deployment_id: i64,
    pub subject_type: String,
    pub external_key: String,
}

impl GetActorByExternalKeyQuery {
    pub fn new(deployment_id: i64, subject_type: String, external_key: String) -> Self {
        Self {
            deployment_id,
            subject_type,
            external_key,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<Actor>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let actor = sqlx::query_as!(
            Actor,
            r#"
            SELECT id, deployment_id, subject_type, external_key, display_name, metadata, created_at, updated_at, archived_at
            FROM actors
            WHERE deployment_id = $1 AND subject_type = $2 AND external_key = $3
            "#,
            self.deployment_id,
            self.subject_type,
            self.external_key,
        )
        .fetch_optional(executor)
        .await?;

        Ok(actor)
    }
}

impl GetActorByIdQuery {
    pub fn new(actor_id: i64, deployment_id: i64) -> Self {
        Self {
            actor_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<Actor>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let actor = sqlx::query_as!(
            Actor,
            r#"
            SELECT id, deployment_id, subject_type, external_key, display_name, metadata, created_at, updated_at, archived_at
            FROM actors
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.actor_id,
            self.deployment_id,
        )
        .fetch_optional(executor)
        .await?;

        Ok(actor)
    }
}

pub struct ListActorsQuery {
    pub deployment_id: i64,
    pub include_archived: bool,
}

impl ListActorsQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            include_archived: false,
        }
    }

    pub fn include_archived(mut self) -> Self {
        self.include_archived = true;
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<Actor>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = if self.include_archived {
            sqlx::query_as!(
                Actor,
                r#"
                SELECT id, deployment_id, subject_type, external_key, display_name, metadata, created_at, updated_at, archived_at
                FROM actors
                WHERE deployment_id = $1
                ORDER BY created_at DESC
                "#,
                self.deployment_id,
            )
            .fetch_all(executor)
            .await?
        } else {
            sqlx::query_as!(
                Actor,
                r#"
                SELECT id, deployment_id, subject_type, external_key, display_name, metadata, created_at, updated_at, archived_at
                FROM actors
                WHERE deployment_id = $1 AND archived_at IS NULL
                ORDER BY created_at DESC
                "#,
                self.deployment_id,
            )
            .fetch_all(executor)
            .await?
        };

        Ok(rows)
    }
}

pub struct GetActorProjectByIdQuery {
    pub project_id: i64,
    pub deployment_id: i64,
}

impl GetActorProjectByIdQuery {
    pub fn new(project_id: i64, deployment_id: i64) -> Self {
        Self {
            project_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ActorProject>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project = sqlx::query_as!(
            ActorProject,
            r#"
            SELECT id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                   review_thread_id, metadata, created_at,
                   updated_at, archived_at
            FROM actor_projects
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.project_id,
            self.deployment_id,
        )
        .fetch_optional(executor)
        .await?;

        Ok(project)
    }
}

pub struct GetActorProjectByNameQuery {
    pub deployment_id: i64,
    pub actor_id: i64,
    pub name: String,
}

impl GetActorProjectByNameQuery {
    pub fn new(deployment_id: i64, actor_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            actor_id,
            name,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ActorProject>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project = sqlx::query_as!(
            ActorProject,
            r#"
            SELECT id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                   review_thread_id, metadata, created_at,
                   updated_at, archived_at
            FROM actor_projects
            WHERE deployment_id = $1 AND actor_id = $2 AND name = $3
            "#,
            self.deployment_id,
            self.actor_id,
            self.name,
        )
        .fetch_optional(executor)
        .await?;

        Ok(project)
    }
}

pub struct ListActorProjectsQuery {
    pub deployment_id: i64,
    pub actor_id: i64,
    pub include_archived: bool,
}

impl ListActorProjectsQuery {
    pub fn new(deployment_id: i64, actor_id: i64) -> Self {
        Self {
            deployment_id,
            actor_id,
            include_archived: false,
        }
    }

    pub fn include_archived(mut self) -> Self {
        self.include_archived = true;
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ActorProject>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = if self.include_archived {
            sqlx::query_as!(
                ActorProject,
                r#"
                SELECT id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                       review_thread_id, metadata, created_at,
                       updated_at, archived_at
                FROM actor_projects
                WHERE deployment_id = $1 AND actor_id = $2
                ORDER BY updated_at DESC
                "#,
                self.deployment_id,
                self.actor_id,
            )
            .fetch_all(executor)
            .await?
        } else {
            sqlx::query_as!(
                ActorProject,
                r#"
                SELECT id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                       review_thread_id, metadata, created_at,
                       updated_at, archived_at
                FROM actor_projects
                WHERE deployment_id = $1 AND actor_id = $2 AND archived_at IS NULL
                ORDER BY updated_at DESC
                "#,
                self.deployment_id,
                self.actor_id,
            )
            .fetch_all(executor)
            .await?
        };

        Ok(rows)
    }
}

pub struct GetAgentThreadByIdQuery {
    pub thread_id: i64,
    pub deployment_id: i64,
}

impl GetAgentThreadByIdQuery {
    pub fn new(thread_id: i64, deployment_id: i64) -> Self {
        Self {
            thread_id,
            deployment_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<AgentThread>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let thread = sqlx::query_as!(
            AgentThread,
            r#"
            SELECT id, deployment_id, actor_id, project_id,
                   title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                   thread_purpose, responsibility,
                   reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                   execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
            FROM agent_threads
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.thread_id,
            self.deployment_id,
        )
        .fetch_optional(executor)
        .await?;

        Ok(thread)
    }
}

pub struct ListAgentThreadsQuery {
    pub deployment_id: i64,
    pub project_id: i64,
    pub include_archived: bool,
}

impl ListAgentThreadsQuery {
    pub fn new(deployment_id: i64, project_id: i64) -> Self {
        Self {
            deployment_id,
            project_id,
            include_archived: false,
        }
    }

    pub fn include_archived(mut self) -> Self {
        self.include_archived = true;
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<AgentThread>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = if self.include_archived {
            sqlx::query_as!(
                AgentThread,
                r#"
                SELECT id, deployment_id, actor_id, project_id,
                       title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                       thread_purpose, responsibility,
                       reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                       execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
                FROM agent_threads
                WHERE deployment_id = $1 AND project_id = $2
                ORDER BY updated_at DESC
                "#,
                self.deployment_id,
                self.project_id,
            )
            .fetch_all(executor)
            .await?
        } else {
            sqlx::query_as!(
                AgentThread,
                r#"
                SELECT id, deployment_id, actor_id, project_id,
                       title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                       thread_purpose, responsibility,
                       reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                       execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
                FROM agent_threads
                WHERE deployment_id = $1 AND project_id = $2 AND archived_at IS NULL
                ORDER BY updated_at DESC
                "#,
                self.deployment_id,
                self.project_id,
            )
            .fetch_all(executor)
            .await?
        };

        Ok(rows)
    }
}

pub struct ListAssignableAgentThreadsQuery {
    pub deployment_id: i64,
    pub project_id: i64,
    pub include_user_facing: bool,
}

impl ListAssignableAgentThreadsQuery {
    pub fn new(deployment_id: i64, project_id: i64) -> Self {
        Self {
            deployment_id,
            project_id,
            include_user_facing: false,
        }
    }

    pub fn include_user_facing(mut self) -> Self {
        self.include_user_facing = true;
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<AgentThread>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = if self.include_user_facing {
            sqlx::query_as!(
                AgentThread,
                r#"
                SELECT id, deployment_id, actor_id, project_id,
                       title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                       thread_purpose, responsibility,
                       reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                       execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
                FROM agent_threads
                WHERE deployment_id = $1
                  AND project_id = $2
                  AND accepts_assignments = true
                  AND archived_at IS NULL
                ORDER BY reusable DESC, updated_at DESC
                "#,
                self.deployment_id,
                self.project_id,
            )
            .fetch_all(executor)
            .await?
        } else {
            sqlx::query_as!(
                AgentThread,
                r#"
                SELECT id, deployment_id, actor_id, project_id,
                       title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                       thread_purpose, responsibility,
                       reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                       execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
                FROM agent_threads
                WHERE deployment_id = $1
                  AND project_id = $2
                  AND thread_purpose <> $3
                  AND accepts_assignments = true
                  AND archived_at IS NULL
                ORDER BY reusable DESC, updated_at DESC
                "#,
                self.deployment_id,
                self.project_id,
                models::agent_thread::purpose::CONVERSATION,
            )
            .fetch_all(executor)
            .await?
        };

        Ok(rows)
    }
}
