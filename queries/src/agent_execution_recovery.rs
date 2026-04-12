use chrono::{DateTime, Utc};
use common::error::AppError;
#[derive(Debug, Clone)]
pub struct StaleClaimedThreadEventCandidate {
    pub deployment_id: i64,
    pub thread_id: i64,
    pub thread_status: String,
    pub thread_updated_at: DateTime<Utc>,
    pub thread_event_id: i64,
    pub board_item_id: Option<i64>,
    pub event_type: String,
    pub claimed_at: Option<DateTime<Utc>>,
    pub execution_run_id: Option<i64>,
    pub execution_run_status: Option<String>,
    pub execution_run_started_at: Option<DateTime<Utc>>,
    pub execution_run_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct StaleExecutionRunCandidate {
    pub deployment_id: i64,
    pub thread_id: i64,
    pub thread_status: String,
    pub thread_updated_at: DateTime<Utc>,
    pub execution_run_id: i64,
    pub execution_run_started_at: DateTime<Utc>,
    pub execution_run_updated_at: DateTime<Utc>,
    pub board_item_id: Option<i64>,
}

pub struct ListStaleClaimedThreadEventsQuery {
    pub stale_before: DateTime<Utc>,
    pub limit: i64,
}

pub struct ListStaleExecutionRunsQuery {
    pub stale_before: DateTime<Utc>,
    pub limit: i64,
}

impl ListStaleClaimedThreadEventsQuery {
    pub fn new(stale_before: DateTime<Utc>, limit: i64) -> Self {
        Self {
            stale_before,
            limit,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<StaleClaimedThreadEventCandidate>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            StaleClaimedThreadEventCandidate,
            r#"
            SELECT
                e.deployment_id,
                e.thread_id,
                t.status AS thread_status,
                t.updated_at AS thread_updated_at,
                e.id AS thread_event_id,
                e.board_item_id,
                e.event_type,
                e.claimed_at AS "claimed_at!",
                e.caused_by_run_id AS execution_run_id,
                r.status AS execution_run_status,
                r.started_at AS execution_run_started_at,
                r.updated_at AS execution_run_updated_at
            FROM thread_events e
            INNER JOIN agent_threads t
                ON t.id = e.thread_id
               AND t.deployment_id = e.deployment_id
            LEFT JOIN execution_runs r
                ON r.id = e.caused_by_run_id
               AND r.deployment_id = e.deployment_id
            WHERE e.status = 'claimed'
              AND e.claimed_at IS NOT NULL
              AND e.claimed_at < $1
            ORDER BY e.claimed_at ASC
            LIMIT $2
            "#,
            self.stale_before,
            self.limit,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows)
    }
}

impl ListStaleExecutionRunsQuery {
    pub fn new(stale_before: DateTime<Utc>, limit: i64) -> Self {
        Self {
            stale_before,
            limit,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<StaleExecutionRunCandidate>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as!(
            StaleExecutionRunCandidate,
            r#"
            SELECT
                r.deployment_id,
                r.thread_id,
                t.status AS thread_status,
                t.updated_at AS thread_updated_at,
                r.id AS execution_run_id,
                r.started_at AS execution_run_started_at,
                r.updated_at AS execution_run_updated_at,
                (
                    SELECT e.board_item_id
                    FROM thread_events e
                    WHERE e.caused_by_run_id = r.id
                    ORDER BY e.created_at DESC
                    LIMIT 1
                ) AS board_item_id
            FROM execution_runs r
            INNER JOIN agent_threads t
                ON t.id = r.thread_id
               AND t.deployment_id = r.deployment_id
            WHERE r.status = 'running'
              AND r.completed_at IS NULL
              AND r.failed_at IS NULL
              AND r.started_at < $1
              AND t.status = 'running'
              AND NOT EXISTS (
                    SELECT 1
                    FROM thread_events e
                    WHERE e.caused_by_run_id = r.id
                      AND e.status = 'claimed'
              )
            ORDER BY r.started_at ASC
            LIMIT $2
            "#,
            self.stale_before,
            self.limit,
        )
        .fetch_all(executor)
        .await?;

        Ok(rows)
    }
}
