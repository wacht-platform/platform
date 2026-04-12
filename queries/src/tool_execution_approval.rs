use common::error::AppError;
use models::ApprovalGrant;

pub struct ListActiveApprovalGrantsForThreadQuery {
    pub deployment_id: i64,
    pub thread_id: i64,
}

impl ListActiveApprovalGrantsForThreadQuery {
    pub fn new(deployment_id: i64, thread_id: i64) -> Self {
        Self {
            deployment_id,
            thread_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<ApprovalGrant>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let approvals = sqlx::query_as!(
            ApprovalGrant,
            r#"
            SELECT
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
            FROM approval_grants
            WHERE deployment_id = $1
              AND status = $2
              AND thread_id = $3
            ORDER BY granted_at ASC
            "#,
            self.deployment_id,
            models::approval::grant_status::ACTIVE,
            self.thread_id
        )
        .fetch_all(executor)
        .await?;

        Ok(approvals)
    }
}
