use crate::Query;
use common::error::AppError;
use common::state::AppState;

pub struct GetOrganizationNotificationRecipientUserIdsQuery {
    pub deployment_id: i64,
    pub organization_id: i64,
}

impl GetOrganizationNotificationRecipientUserIdsQuery {
    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
        }
    }
}

impl Query for GetOrganizationNotificationRecipientUserIdsQuery {
    type Output = Vec<i64>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT om.user_id
            FROM organization_memberships om
            JOIN organizations o ON o.id = om.organization_id
            WHERE o.deployment_id = $1
              AND om.organization_id = $2
              AND om.deleted_at IS NULL
            "#,
            self.deployment_id,
            self.organization_id
        )
        .fetch_all(&state.db_pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.user_id).collect())
    }
}

pub struct GetWorkspaceNotificationRecipientUserIdsQuery {
    pub deployment_id: i64,
    pub workspace_id: i64,
}

impl GetWorkspaceNotificationRecipientUserIdsQuery {
    pub fn new(deployment_id: i64, workspace_id: i64) -> Self {
        Self {
            deployment_id,
            workspace_id,
        }
    }
}

impl Query for GetWorkspaceNotificationRecipientUserIdsQuery {
    type Output = Vec<i64>;

    async fn execute(&self, state: &AppState) -> Result<Self::Output, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT wm.user_id
            FROM workspace_memberships wm
            JOIN workspaces w ON w.id = wm.workspace_id
            WHERE w.deployment_id = $1
              AND wm.workspace_id = $2
              AND wm.deleted_at IS NULL
            "#,
            self.deployment_id,
            self.workspace_id
        )
        .fetch_all(&state.db_pool)
        .await?;

        Ok(rows.into_iter().map(|r| r.user_id).collect())
    }
}
