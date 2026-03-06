use common::error::AppError;

pub struct GetOrganizationNotificationRecipientUserIdsQuery {
    deployment_id: i64,
    organization_id: i64,
}

#[derive(Default)]
pub struct GetOrganizationNotificationRecipientUserIdsQueryBuilder {
    deployment_id: Option<i64>,
    organization_id: Option<i64>,
}

impl GetOrganizationNotificationRecipientUserIdsQuery {
    pub fn builder() -> GetOrganizationNotificationRecipientUserIdsQueryBuilder {
        GetOrganizationNotificationRecipientUserIdsQueryBuilder::default()
    }

    pub fn new(deployment_id: i64, organization_id: i64) -> Self {
        Self {
            deployment_id,
            organization_id,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows.into_iter().map(|r| r.user_id).collect())
    }
}

impl GetOrganizationNotificationRecipientUserIdsQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn organization_id(mut self, organization_id: i64) -> Self {
        self.organization_id = Some(organization_id);
        self
    }

    pub fn build(self) -> Result<GetOrganizationNotificationRecipientUserIdsQuery, AppError> {
        Ok(GetOrganizationNotificationRecipientUserIdsQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            organization_id: self
                .organization_id
                .ok_or_else(|| AppError::Validation("organization_id is required".into()))?,
        })
    }
}

pub struct GetWorkspaceNotificationRecipientUserIdsQuery {
    deployment_id: i64,
    workspace_id: i64,
}

#[derive(Default)]
pub struct GetWorkspaceNotificationRecipientUserIdsQueryBuilder {
    deployment_id: Option<i64>,
    workspace_id: Option<i64>,
}

impl GetWorkspaceNotificationRecipientUserIdsQuery {
    pub fn builder() -> GetWorkspaceNotificationRecipientUserIdsQueryBuilder {
        GetWorkspaceNotificationRecipientUserIdsQueryBuilder::default()
    }

    pub fn new(deployment_id: i64, workspace_id: i64) -> Self {
        Self {
            deployment_id,
            workspace_id,
        }
    }

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<Vec<i64>, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .fetch_all(&mut *conn)
        .await?;

        Ok(rows.into_iter().map(|r| r.user_id).collect())
    }
}

impl GetWorkspaceNotificationRecipientUserIdsQueryBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn workspace_id(mut self, workspace_id: i64) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }

    pub fn build(self) -> Result<GetWorkspaceNotificationRecipientUserIdsQuery, AppError> {
        Ok(GetWorkspaceNotificationRecipientUserIdsQuery {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".into()))?,
            workspace_id: self
                .workspace_id
                .ok_or_else(|| AppError::Validation("workspace_id is required".into()))?,
        })
    }
}
