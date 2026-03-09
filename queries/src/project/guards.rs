use super::*;
pub struct BillingAccountForOwnerLockResult {
    pub id: i64,
    pub status: String,
    pub pulse_usage_disabled: bool,
    pub max_projects_per_account: i64,
    pub max_staging_deployments_per_project: i64,
}

#[derive(Default)]
pub struct BillingAccountForOwnerLockQuery {
    owner_id: Option<String>,
}

impl BillingAccountForOwnerLockQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn owner_id(mut self, owner_id: impl Into<String>) -> Self {
        self.owner_id = Some(owner_id.into());
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<BillingAccountForOwnerLockResult>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let owner_id = self
            .owner_id
            .as_deref()
            .ok_or_else(|| AppError::Validation("owner_id is required".to_string()))?;

        let row = sqlx::query!(
            r#"
            SELECT
                id,
                status,
                COALESCE(pulse_usage_disabled, false) AS "pulse_usage_disabled!",
                COALESCE((to_jsonb(billing_accounts) ->> 'max_projects_per_account')::BIGINT, 10) AS "max_projects_per_account!",
                COALESCE((to_jsonb(billing_accounts) ->> 'max_staging_deployments_per_project')::BIGINT, 3) AS "max_staging_deployments_per_project!"
            FROM billing_accounts
            WHERE owner_id = $1
            FOR UPDATE
            "#,
            owner_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| BillingAccountForOwnerLockResult {
            id: r.id,
            status: r.status,
            pulse_usage_disabled: r.pulse_usage_disabled,
            max_projects_per_account: r.max_projects_per_account,
            max_staging_deployments_per_project: r.max_staging_deployments_per_project,
        }))
    }
}

#[derive(Default)]
pub struct ProjectsCountByBillingAccountQuery {
    billing_account_id: Option<i64>,
}

impl ProjectsCountByBillingAccountQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn billing_account_id(mut self, billing_account_id: i64) -> Self {
        self.billing_account_id = Some(billing_account_id);
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<i64, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let billing_account_id = self
            .billing_account_id
            .ok_or_else(|| AppError::Validation("billing_account_id is required".to_string()))?;

        let row = sqlx::query!(
            r#"
            SELECT COUNT(*)::BIGINT as "count!"
            FROM projects
            WHERE billing_account_id = $1
              AND deleted_at IS NULL
            "#,
            billing_account_id
        )
        .fetch_one(executor)
        .await?;

        Ok(row.count)
    }
}

pub struct ProjectWithBillingForStagingRow {
    pub name: String,
    pub status: String,
    pub pulse_usage_disabled: bool,
    pub max_staging_deployments_per_project: i64,
}

#[derive(Default)]
pub struct ProjectWithBillingForStagingQuery {
    project_id: Option<i64>,
}

impl ProjectWithBillingForStagingQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectWithBillingForStagingRow>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = sqlx::query!(
            r#"
            SELECT
                p.name,
                ba.status,
                COALESCE(ba.pulse_usage_disabled, false) AS "pulse_usage_disabled!",
                COALESCE((to_jsonb(ba) ->> 'max_staging_deployments_per_project')::BIGINT, 3) AS "max_staging_deployments_per_project!"
            FROM projects p
            JOIN billing_accounts ba ON p.billing_account_id = ba.id
            WHERE p.id = $1 AND p.deleted_at IS NULL
            "#,
            project_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| ProjectWithBillingForStagingRow {
            name: r.name,
            status: r.status,
            pulse_usage_disabled: r.pulse_usage_disabled,
            max_staging_deployments_per_project: r.max_staging_deployments_per_project,
        }))
    }
}

#[derive(Default)]
pub struct StagingDeploymentCountByProjectQuery {
    project_id: Option<i64>,
}

impl StagingDeploymentCountByProjectQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<i64, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = sqlx::query!(
            "SELECT COUNT(*) as count FROM deployments WHERE project_id = $1 AND mode = 'staging' AND deleted_at IS NULL",
            project_id
        )
        .fetch_one(executor)
        .await?;

        Ok(row.count.unwrap_or(0))
    }
}

pub struct ProjectForProductionRow {
    pub name: String,
    pub status: String,
}

#[derive(Default)]
pub struct ProjectForProductionQuery {
    project_id: Option<i64>,
}

impl ProjectForProductionQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ProjectForProductionRow>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = ProjectWithBillingForStagingQuery::builder()
            .project_id(project_id)
            .execute_with_db(executor)
            .await?;

        Ok(row.map(|r| ProjectForProductionRow {
            name: r.name,
            status: r.status,
        }))
    }
}

#[derive(Default)]
pub struct ExistingProductionDeploymentQuery {
    project_id: Option<i64>,
}

impl ExistingProductionDeploymentQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Option<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let row = sqlx::query!(
            "SELECT id FROM deployments WHERE project_id = $1 AND mode = 'production' AND deleted_at IS NULL",
            project_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| r.id))
    }
}

pub struct ExistingDomainDeploymentRow {
    pub id: i64,
}

#[derive(Default)]
pub struct ExistingDomainDeploymentQuery {
    custom_domain: Option<String>,
}

impl ExistingDomainDeploymentQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn custom_domain(mut self, custom_domain: impl Into<String>) -> Self {
        self.custom_domain = Some(custom_domain.into());
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<ExistingDomainDeploymentRow>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let custom_domain = self
            .custom_domain
            .as_deref()
            .ok_or_else(|| AppError::Validation("custom_domain is required".to_string()))?;

        let row = sqlx::query!(
            "SELECT id FROM deployments WHERE (backend_host = $1 OR frontend_host = $2 OR mail_from_host = $3) AND deleted_at IS NULL",
            format!("frontend.{}", custom_domain),
            format!("accounts.{}", custom_domain),
            custom_domain
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| ExistingDomainDeploymentRow { id: r.id }))
    }
}
