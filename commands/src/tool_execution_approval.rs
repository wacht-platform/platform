use chrono::Utc;
use common::{HasDbRouter, HasIdProvider, error::AppError};
use models::{ApprovalGrant, ToolApprovalMode};

pub struct ApprovalGrantRequest {
    pub tool_id: i64,
    pub mode: ToolApprovalMode,
}

pub struct GrantApprovalGrantsForThreadCommand {
    pub deployment_id: i64,
    pub thread_id: i64,
    pub grants: Vec<ApprovalGrantRequest>,
}

impl GrantApprovalGrantsForThreadCommand {
    pub fn new(deployment_id: i64, thread_id: i64, grants: Vec<ApprovalGrantRequest>) -> Self {
        Self {
            deployment_id,
            thread_id,
            grants,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<ApprovalGrant>, AppError>
    where
        D: HasDbRouter + HasIdProvider,
    {
        let mut tx = deps
            .writer_pool()
            .begin()
            .await
            .map_err(AppError::Database)?;
        let now = Utc::now();
        let mut approvals = Vec::with_capacity(self.grants.len());

        for grant in self.grants {
            let approval_id = deps.id_provider().next_id()? as i64;
            let grant_scope = match grant.mode {
                ToolApprovalMode::AllowOnce => models::approval::grant_scope::ONCE,
                ToolApprovalMode::AllowAlways => models::approval::grant_scope::THREAD,
            };

            let approval = sqlx::query_as!(
                ApprovalGrant,
                r#"
                INSERT INTO approval_grants (
                    id,
                    deployment_id,
                    policy_id,
                    actor_id,
                    project_id,
                    thread_id,
                    tool_id,
                    granted_by_message_id,
                    grant_scope,
                    status,
                    granted_at,
                    expires_at,
                    consumed_at,
                    consumed_by_run_id,
                    metadata
                )
                VALUES (
                    $1, $2, NULL, NULL, NULL, $3, $4, NULL, $5, $6, $7, NULL, NULL, NULL,
                    '{}'::jsonb
                )
                RETURNING
                    id,
                    deployment_id,
                    policy_id,
                    actor_id,
                    project_id,
                    thread_id,
                    tool_id,
                    granted_by_message_id,
                    grant_scope,
                    status,
                    granted_at,
                    expires_at,
                    consumed_at,
                    consumed_by_run_id,
                    metadata
                "#,
                approval_id,
                self.deployment_id,
                self.thread_id,
                grant.tool_id,
                grant_scope,
                models::approval::grant_status::ACTIVE,
                now,
            )
            .fetch_one(&mut *tx)
            .await?;

            approvals.push(approval);
        }

        tx.commit().await.map_err(AppError::Database)?;
        Ok(approvals)
    }
}

pub struct ConsumeOnceApprovalGrantForThreadCommand {
    pub deployment_id: i64,
    pub thread_id: i64,
    pub tool_id: i64,
}

impl ConsumeOnceApprovalGrantForThreadCommand {
    pub fn new(deployment_id: i64, thread_id: i64, tool_id: i64) -> Self {
        Self {
            deployment_id,
            thread_id,
            tool_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let consumed_id = sqlx::query_scalar!(
            r#"
            WITH next_approval AS (
                SELECT id
                FROM approval_grants
                WHERE deployment_id = $1
                  AND thread_id = $2
                  AND tool_id = $3
                  AND grant_scope = $4
                  AND status = $5
                ORDER BY granted_at ASC
                LIMIT 1
            )
            UPDATE approval_grants approvals
            SET status = $6,
                consumed_at = $7,
                consumed_by_run_id = NULL
            FROM next_approval
            WHERE approvals.id = next_approval.id
            RETURNING approvals.id
            "#,
            self.deployment_id,
            self.thread_id,
            self.tool_id,
            models::approval::grant_scope::ONCE,
            models::approval::grant_status::ACTIVE,
            models::approval::grant_status::CONSUMED,
            now,
        )
        .fetch_optional(executor)
        .await?;

        Ok(consumed_id)
    }
}
