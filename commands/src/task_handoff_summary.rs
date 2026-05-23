use chrono::Utc;
use common::error::AppError;
use models::TaskHandoffSummary;

pub struct CreateTaskHandoffSummaryCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub board_item_id: i64,
    pub thread_id: i64,
    pub assignment_id: Option<i64>,
    pub execution_run_id: Option<i64>,
    pub assignment_role: String,
    pub outcome: String,
    pub summary: String,
    pub artifacts: Option<serde_json::Value>,
    pub blockers: Option<serde_json::Value>,
    pub next_actions: Option<serde_json::Value>,
    pub metadata: Option<serde_json::Value>,
}

impl CreateTaskHandoffSummaryCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        board_item_id: i64,
        thread_id: i64,
        assignment_role: impl Into<String>,
        outcome: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            id,
            deployment_id,
            board_item_id,
            thread_id,
            assignment_id: None,
            execution_run_id: None,
            assignment_role: assignment_role.into(),
            outcome: outcome.into(),
            summary: summary.into(),
            artifacts: None,
            blockers: None,
            next_actions: None,
            metadata: None,
        }
    }

    pub fn with_assignment_id(mut self, assignment_id: i64) -> Self {
        self.assignment_id = Some(assignment_id);
        self
    }

    pub fn with_execution_run_id(mut self, execution_run_id: i64) -> Self {
        self.execution_run_id = Some(execution_run_id);
        self
    }

    pub fn with_artifacts(mut self, artifacts: serde_json::Value) -> Self {
        self.artifacts = Some(artifacts);
        self
    }

    pub fn with_blockers(mut self, blockers: serde_json::Value) -> Self {
        self.blockers = Some(blockers);
        self
    }

    pub fn with_next_actions(mut self, next_actions: serde_json::Value) -> Self {
        self.next_actions = Some(next_actions);
        self
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<TaskHandoffSummary, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let row = sqlx::query_as!(
            TaskHandoffSummary,
            r#"
            INSERT INTO task_handoff_summaries (
                id, deployment_id, board_item_id, thread_id,
                assignment_id, execution_run_id,
                assignment_role, outcome, summary,
                artifacts, blockers, next_actions, metadata,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, $6,
                $7, $8, $9,
                $10, $11, $12, $13,
                $14, $14
            )
            RETURNING
                id, deployment_id, board_item_id, thread_id,
                assignment_id, execution_run_id,
                assignment_role, outcome, summary,
                artifacts, blockers, next_actions, metadata,
                created_at, updated_at
            "#,
            self.id,
            self.deployment_id,
            self.board_item_id,
            self.thread_id,
            self.assignment_id,
            self.execution_run_id,
            self.assignment_role,
            self.outcome,
            self.summary,
            self.artifacts,
            self.blockers,
            self.next_actions,
            self.metadata,
            now,
        )
        .fetch_one(executor)
        .await?;
        Ok(row)
    }
}
