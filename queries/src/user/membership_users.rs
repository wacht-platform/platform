use common::error::AppError;

/// Resolves the user behind an organization membership id (used to re-index that
/// user's search row when their org membership/roles change). Returns None if the
/// membership doesn't exist.
pub struct GetOrgMembershipUserIdQuery {
    pub membership_id: i64,
}

impl GetOrgMembershipUserIdQuery {
    pub fn new(membership_id: i64) -> Self {
        Self { membership_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query_scalar!(
            "SELECT user_id FROM organization_memberships WHERE id = $1",
            self.membership_id
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)
    }
}

/// Resolves the user behind a workspace membership id.
pub struct GetWorkspaceMembershipUserIdQuery {
    pub membership_id: i64,
}

impl GetWorkspaceMembershipUserIdQuery {
    pub fn new(membership_id: i64) -> Self {
        Self { membership_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query_scalar!(
            "SELECT user_id FROM workspace_memberships WHERE id = $1",
            self.membership_id
        )
        .fetch_optional(executor)
        .await
        .map_err(AppError::Database)
    }
}
