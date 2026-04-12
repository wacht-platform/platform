use chrono::Utc;
use common::error::AppError;
use models::{Actor, ActorProject, AgentThread, ExecutionRun};

pub struct CreateActorCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub subject_type: String,
    pub external_key: String,
    pub display_name: Option<String>,
    pub metadata: serde_json::Value,
}

impl CreateActorCommand {
    pub fn new(id: i64, deployment_id: i64, subject_type: String, external_key: String) -> Self {
        Self {
            id,
            deployment_id,
            subject_type,
            external_key,
            display_name: None,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_display_name(mut self, display_name: String) -> Self {
        self.display_name = Some(display_name);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Actor, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let actor = sqlx::query_as!(
            Actor,
            r#"
            INSERT INTO actors (
                id, deployment_id, subject_type, external_key, display_name, metadata,
                created_at, updated_at, archived_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $7, NULL)
            RETURNING id, deployment_id, subject_type, external_key, display_name, metadata,
                      created_at, updated_at, archived_at
            "#,
            self.id,
            self.deployment_id,
            self.subject_type,
            self.external_key,
            self.display_name,
            self.metadata,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(actor)
    }
}

pub struct CreateActorProjectCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub actor_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub coordinator_thread_id: Option<i64>,
    pub review_thread_id: Option<i64>,
    pub metadata: serde_json::Value,
}

impl CreateActorProjectCommand {
    pub fn new(id: i64, deployment_id: i64, actor_id: i64, name: String, status: String) -> Self {
        Self {
            id,
            deployment_id,
            actor_id,
            name,
            description: None,
            status,
            coordinator_thread_id: None,
            review_thread_id: None,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ActorProject, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let project = sqlx::query_as!(
            ActorProject,
            r#"
            INSERT INTO actor_projects (
                id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                review_thread_id, metadata, created_at, updated_at, archived_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10, NULL)
            RETURNING id, deployment_id, actor_id, name, description, status, coordinator_thread_id,
                      review_thread_id, metadata,
                      created_at, updated_at, archived_at
            "#,
            self.id,
            self.deployment_id,
            self.actor_id,
            self.name,
            self.description,
            self.status,
            self.coordinator_thread_id,
            self.review_thread_id,
            self.metadata,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(project)
    }
}

pub struct CreateAgentThreadCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub actor_id: i64,
    pub project_id: i64,
    pub title: String,
    pub thread_purpose: String,
    pub responsibility: Option<String>,
    pub reusable: bool,
    pub accepts_assignments: bool,
    pub capability_tags: Vec<String>,
    pub status: String,
    pub system_instructions: Option<String>,
    pub metadata: serde_json::Value,
}

impl CreateAgentThreadCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        actor_id: i64,
        project_id: i64,
        title: String,
        thread_purpose: String,
        status: String,
    ) -> Self {
        Self {
            id,
            deployment_id,
            actor_id,
            project_id,
            title,
            thread_purpose,
            responsibility: None,
            reusable: false,
            accepts_assignments: false,
            capability_tags: Vec::new(),
            status,
            system_instructions: None,
            metadata: serde_json::json!({}),
        }
    }

    pub fn with_thread_purpose(mut self, thread_purpose: String) -> Self {
        self.thread_purpose = thread_purpose;
        self
    }

    pub fn with_responsibility(mut self, responsibility: String) -> Self {
        self.responsibility = Some(responsibility);
        self
    }

    pub fn mark_reusable(mut self) -> Self {
        self.reusable = true;
        self
    }

    pub fn allow_assignments(mut self) -> Self {
        self.accepts_assignments = true;
        self
    }

    pub fn with_capability_tags(mut self, capability_tags: Vec<String>) -> Self {
        self.capability_tags = capability_tags;
        self
    }

    pub fn with_system_instructions(mut self, system_instructions: String) -> Self {
        self.system_instructions = Some(system_instructions);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AgentThread, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let thread = sqlx::query_as!(
            AgentThread,
            r#"
            INSERT INTO agent_threads (
                id, deployment_id, actor_id, project_id,
                title, thread_purpose, responsibility, reusable, accepts_assignments,
                capability_tags, status, system_instructions, last_activity_at, completed_at,
                execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6, $7, $8, $9,
                $10, $11, $12, $14, NULL,
                NULL, 0, $13, $14, $14, NULL
            )
            RETURNING id, deployment_id, actor_id, project_id,
                      title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                      thread_purpose, responsibility,
                      reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                      execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
            "#,
            self.id,
            self.deployment_id,
            self.actor_id,
            self.project_id,
            self.title,
            self.thread_purpose,
            self.responsibility,
            self.reusable,
            self.accepts_assignments,
            &self.capability_tags,
            self.status,
            self.system_instructions,
            self.metadata,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(thread)
    }
}

pub struct CreateExecutionRunCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub thread_id: i64,
    pub agent_id: Option<i64>,
    pub status: String,
}

pub struct UpdateAgentThreadCommand {
    pub thread_id: i64,
    pub deployment_id: i64,
    pub title: Option<String>,
    pub responsibility: Option<Option<String>>,
    pub reusable: Option<bool>,
    pub accepts_assignments: Option<bool>,
    pub capability_tags: Option<Vec<String>>,
    pub system_instructions: Option<Option<String>>,
    pub metadata: Option<serde_json::Value>,
}

impl UpdateAgentThreadCommand {
    pub fn new(thread_id: i64, deployment_id: i64) -> Self {
        Self {
            thread_id,
            deployment_id,
            title: None,
            responsibility: None,
            reusable: None,
            accepts_assignments: None,
            capability_tags: None,
            system_instructions: None,
            metadata: None,
        }
    }

    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    pub fn with_responsibility(mut self, responsibility: Option<String>) -> Self {
        self.responsibility = Some(responsibility);
        self
    }

    pub fn with_reusable(mut self, reusable: bool) -> Self {
        self.reusable = Some(reusable);
        self
    }

    pub fn with_accepts_assignments(mut self, accepts_assignments: bool) -> Self {
        self.accepts_assignments = Some(accepts_assignments);
        self
    }

    pub fn with_capability_tags(mut self, capability_tags: Vec<String>) -> Self {
        self.capability_tags = Some(capability_tags);
        self
    }

    pub fn with_system_instructions(mut self, system_instructions: Option<String>) -> Self {
        self.system_instructions = Some(system_instructions);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AgentThread, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let thread = sqlx::query_as!(
            AgentThread,
            r#"
            UPDATE agent_threads
            SET
                title = COALESCE($3, title),
                responsibility = COALESCE($4, responsibility),
                reusable = COALESCE($5, reusable),
                accepts_assignments = COALESCE($6, accepts_assignments),
                capability_tags = COALESCE($7, capability_tags),
                system_instructions = COALESCE($8, system_instructions),
                metadata = COALESCE($9, metadata),
                updated_at = $10
            WHERE id = $1
              AND deployment_id = $2
              AND archived_at IS NULL
            RETURNING id, deployment_id, actor_id, project_id,
                      title, thread_purpose as "thread_kind!", CASE WHEN thread_purpose = 'conversation' THEN 'user_facing' ELSE 'internal' END as "thread_visibility!",
                      thread_purpose, responsibility,
                      reusable, accepts_assignments, capability_tags, status, system_instructions, last_activity_at, completed_at,
                      execution_state, next_event_sequence, metadata, created_at, updated_at, archived_at
            "#,
            self.thread_id,
            self.deployment_id,
            self.title,
            self.responsibility.flatten(),
            self.reusable,
            self.accepts_assignments,
            self.capability_tags.as_deref(),
            self.system_instructions.flatten(),
            self.metadata,
            now,
        )
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound("Thread not found".to_string()))?;

        Ok(thread)
    }
}

impl CreateExecutionRunCommand {
    pub fn new(id: i64, deployment_id: i64, thread_id: i64, status: String) -> Self {
        Self {
            id,
            deployment_id,
            thread_id,
            agent_id: None,
            status,
        }
    }

    pub fn with_agent_id(mut self, agent_id: i64) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ExecutionRun, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let run = sqlx::query_as!(
            ExecutionRun,
            r#"
            WITH inserted_run AS (
                INSERT INTO execution_runs (
                    id, deployment_id, thread_id, agent_id, status,
                    started_at, completed_at, failed_at, created_at, updated_at
                ) VALUES (
                    $1, $2, $3, $4, $5,
                    $6, NULL, NULL, $6, $6
                )
                RETURNING id, deployment_id, thread_id, agent_id, status,
                          started_at, completed_at, failed_at, created_at, updated_at
            ),
            updated_thread AS (
                UPDATE agent_threads
                SET updated_at = $6,
                    status = $5
                WHERE id = $3 AND deployment_id = $2
            )
            SELECT id, deployment_id, thread_id, agent_id, status,
                   started_at, completed_at, failed_at, created_at, updated_at
            FROM inserted_run
            "#,
            self.id,
            self.deployment_id,
            self.thread_id,
            self.agent_id,
            self.status,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(run)
    }
}

pub struct UpdateExecutionRunStateCommand {
    pub execution_run_id: i64,
    pub deployment_id: i64,
    pub status: Option<String>,
    pub completed: bool,
    pub failed: bool,
}

impl UpdateExecutionRunStateCommand {
    pub fn new(execution_run_id: i64, deployment_id: i64) -> Self {
        Self {
            execution_run_id,
            deployment_id,
            status: None,
            completed: false,
            failed: false,
        }
    }

    pub fn with_status(mut self, status: String) -> Self {
        self.status = Some(status);
        self
    }

    pub fn mark_completed(mut self) -> Self {
        self.completed = true;
        self
    }

    pub fn mark_failed(mut self) -> Self {
        self.failed = true;
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ExecutionRun, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let row = sqlx::query_as!(
            ExecutionRun,
            r#"
            WITH updated_run AS (
                UPDATE execution_runs
                SET status = COALESCE($1, status),
                    completed_at = CASE WHEN $2 THEN $4 ELSE completed_at END,
                    failed_at = CASE WHEN $3 THEN $4 ELSE failed_at END,
                    updated_at = $4
                WHERE id = $5 AND deployment_id = $6
                RETURNING id, deployment_id, thread_id, agent_id, status,
                          started_at, completed_at, failed_at, created_at, updated_at
            ),
            updated_thread AS (
                UPDATE agent_threads
                SET updated_at = $4,
                    status = updated_run.status
                FROM updated_run
                WHERE agent_threads.id = updated_run.thread_id
                  AND agent_threads.deployment_id = updated_run.deployment_id
            )
            SELECT id, deployment_id, thread_id, agent_id, status,
                   started_at, completed_at, failed_at, created_at, updated_at
            FROM updated_run
            "#,
            self.status,
            self.completed,
            self.failed,
            now,
            self.execution_run_id,
            self.deployment_id,
        )
        .fetch_one(executor)
        .await?;

        Ok(row)
    }
}
