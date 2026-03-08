use std::collections::BTreeMap;

use common::{capabilities::HasDbRouter, db_router::ReadConsistency, error::AppError};
use models::{Deployment, ProjectWithDeployments};
use sqlx::{Row, query};

#[allow(dead_code)]
pub struct GetProjectsWithDeploymentQuery {
    owner_ids: Vec<String>,
    consistency: ReadConsistency,
}

impl GetProjectsWithDeploymentQuery {
    pub fn new() -> Self {
        GetProjectsWithDeploymentQuery {
            owner_ids: Vec::new(),
            consistency: ReadConsistency::Eventual,
        }
    }

    pub fn for_owner(owner_id: String) -> Self {
        GetProjectsWithDeploymentQuery {
            owner_ids: vec![owner_id],
            consistency: ReadConsistency::Eventual,
        }
    }

    pub fn for_owners(owner_ids: Vec<String>) -> Self {
        GetProjectsWithDeploymentQuery {
            owner_ids,
            consistency: ReadConsistency::Eventual,
        }
    }

    pub fn for_user_or_organization(user_id: String, org_id: Option<String>) -> Self {
        let mut owner_ids = Vec::new();
        if let Some(org_id) = org_id {
            owner_ids.push(org_id);
        } else {
            owner_ids.push(user_id);
        }
        GetProjectsWithDeploymentQuery {
            owner_ids,
            consistency: ReadConsistency::Eventual,
        }
    }

    pub fn with_consistency(mut self, consistency: ReadConsistency) -> Self {
        self.consistency = consistency;
        self
    }
}

pub struct BillingAccountForOwnerLockResult {
    pub id: i64,
    pub status: String,
    pub pulse_usage_disabled: bool,
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
            "SELECT id, status, COALESCE(pulse_usage_disabled, false) AS \"pulse_usage_disabled!\" FROM billing_accounts WHERE owner_id = $1 FOR UPDATE",
            owner_id
        )
        .fetch_optional(executor)
        .await?;

        Ok(row.map(|r| BillingAccountForOwnerLockResult {
            id: r.id,
            status: r.status,
            pulse_usage_disabled: r.pulse_usage_disabled,
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
            SELECT p.name, ba.status, COALESCE(ba.pulse_usage_disabled, false) AS "pulse_usage_disabled!"
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

pub struct DeploymentByIdRow {
    pub id: i64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub maintenance_mode: bool,
    pub backend_host: String,
    pub frontend_host: String,
    pub publishable_key: String,
    pub project_id: i64,
    pub mode: String,
    pub mail_from_host: String,
    pub domain_verification_records: Option<serde_json::Value>,
    pub email_verification_records: Option<serde_json::Value>,
    pub email_provider: String,
    pub custom_smtp_config: Option<serde_json::Value>,
}

#[derive(Default)]
pub struct DeploymentByIdQuery {
    deployment_id: Option<i64>,
}

impl DeploymentByIdQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<DeploymentByIdRow, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let deployment_id = self
            .deployment_id
            .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?;

        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deleted_at,
                   maintenance_mode, backend_host, frontend_host, publishable_key,
                   project_id, mode, mail_from_host,
                   domain_verification_records::jsonb as domain_verification_records,
                   email_verification_records::jsonb as email_verification_records,
                   email_provider, custom_smtp_config::jsonb as custom_smtp_config
            FROM deployments
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            deployment_id
        )
        .fetch_one(executor)
        .await?;

        Ok(DeploymentByIdRow {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            maintenance_mode: row.maintenance_mode,
            backend_host: row.backend_host,
            frontend_host: row.frontend_host,
            publishable_key: row.publishable_key,
            project_id: row.project_id,
            mode: row.mode,
            mail_from_host: row.mail_from_host,
            domain_verification_records: row.domain_verification_records,
            email_verification_records: row.email_verification_records,
            email_provider: row.email_provider,
            custom_smtp_config: row.custom_smtp_config,
        })
    }
}

#[derive(Default)]
pub struct ActiveDeploymentIdsByProjectQuery {
    project_id: Option<i64>,
}

impl ActiveDeploymentIdsByProjectQuery {
    pub fn builder() -> Self {
        Self::default()
    }

    pub fn project_id(mut self, project_id: i64) -> Self {
        self.project_id = Some(project_id);
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<i64>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let project_id = self
            .project_id
            .ok_or_else(|| AppError::Validation("project_id is required".to_string()))?;

        let rows = sqlx::query!(
            r#"
            SELECT id FROM deployments
            WHERE project_id = $1 AND deleted_at IS NULL
            "#,
            project_id
        )
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(|r| r.id).collect())
    }
}

impl GetProjectsWithDeploymentQuery {
    fn create_deployment_from_row(row: &sqlx::postgres::PgRow) -> Result<Deployment, AppError> {
        Ok(Deployment {
            id: row
                .get::<Option<i64>, _>("deployment_id")
                .unwrap_or_default(),
            created_at: row
                .get::<Option<_>, _>("deployment_created_at")
                .unwrap_or_default(),
            updated_at: row
                .get::<Option<_>, _>("deployment_updated_at")
                .unwrap_or_default(),
            maintenance_mode: row
                .get::<Option<bool>, _>("deployment_maintenance_mode")
                .unwrap_or_default(),
            backend_host: row
                .get::<Option<String>, _>("deployment_backend_host")
                .unwrap_or_default(),
            frontend_host: row
                .get::<Option<String>, _>("deployment_frontend_host")
                .unwrap_or_default(),
            publishable_key: row
                .get::<Option<String>, _>("deployment_publishable_key")
                .unwrap_or_default(),
            project_id: row
                .get::<Option<i64>, _>("deployment_project_id")
                .unwrap_or_default(),
            mode: row
                .get::<Option<String>, _>("deployment_mode")
                .unwrap_or_default()
                .into(),
            mail_from_host: row
                .get::<Option<String>, _>("deployment_mail_from_host")
                .unwrap_or_default(),
            domain_verification_records: row
                .get::<Option<serde_json::Value>, _>("deployment_domain_verification_records")
                .map(|v| {
                    serde_json::from_value(v).map_err(|e| {
                        AppError::Internal(format!(
                            "Invalid deployment_domain_verification_records JSON: {}",
                            e
                        ))
                    })
                })
                .transpose()?,
            email_verification_records: row
                .get::<Option<serde_json::Value>, _>("deployment_email_verification_records")
                .map(|v| {
                    serde_json::from_value(v).map_err(|e| {
                        AppError::Internal(format!(
                            "Invalid deployment_email_verification_records JSON: {}",
                            e
                        ))
                    })
                })
                .transpose()?,
            email_provider: row
                .get::<Option<String>, _>("deployment_email_provider")
                .map(models::EmailProvider::from)
                .unwrap_or_default(),
            custom_smtp_config: row
                .get::<Option<serde_json::Value>, _>("deployment_custom_smtp_config")
                .map(|v| {
                    serde_json::from_value(v).map_err(|e| {
                        AppError::Internal(format!(
                            "Invalid deployment_custom_smtp_config JSON: {}",
                            e
                        ))
                    })
                })
                .transpose()?
                .map(|mut c: models::CustomSmtpConfig| {
                    c.password = String::new();
                    c
                }),
        })
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<ProjectWithDeployments>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut query_str = r#"
            SELECT
                p.id, p.created_at, p.updated_at, p.name, p.image_url,
                p.owner_id, p.billing_account_id,
                d.id as deployment_id, d.created_at as deployment_created_at,
                d.updated_at as deployment_updated_at,
                d.maintenance_mode as deployment_maintenance_mode, d.backend_host as deployment_backend_host,
                d.frontend_host as deployment_frontend_host,
                d.publishable_key as deployment_publishable_key,
                d.project_id as deployment_project_id, d.mode as deployment_mode,
                d.mail_from_host as deployment_mail_from_host,
                d.domain_verification_records::jsonb as deployment_domain_verification_records,
                d.email_verification_records::jsonb as deployment_email_verification_records,
                d.email_provider as deployment_email_provider,
                d.custom_smtp_config::jsonb as deployment_custom_smtp_config
            FROM projects p
            LEFT JOIN deployments d ON p.id = d.project_id AND d.deleted_at IS NULL
        "#
        .to_string();

        if !self.owner_ids.is_empty() {
            query_str.push_str(" WHERE p.owner_id = ANY($1)");
        }

        query_str.push_str(" ORDER BY p.id DESC");

        let rows = if self.owner_ids.is_empty() {
            query(&query_str).fetch_all(executor).await?
        } else {
            query(&query_str)
                .bind(&self.owner_ids)
                .fetch_all(executor)
                .await?
        };

        let mut projects_map: BTreeMap<i64, ProjectWithDeployments> = BTreeMap::new();

        for row in rows {
            let project_id = row.get("id");

            if let Some(project) = projects_map.get_mut(&project_id) {
                if row.get::<Option<i64>, _>("deployment_id").is_some() {
                    project.deployments.push(Self::create_deployment_from_row(&row)?);
                }
            } else {
                let mut deployments = Vec::new();
                if row.get::<Option<i64>, _>("deployment_id").is_some() {
                    deployments.push(Self::create_deployment_from_row(&row)?);
                }

                projects_map.insert(
                    project_id,
                    ProjectWithDeployments {
                        id: project_id,
                        image_url: row.get("image_url"),
                        created_at: row.get("created_at"),
                        updated_at: row.get("updated_at"),
                        name: row.get("name"),
                        owner_id: row.get("owner_id"),
                        billing_account_id: row.get("billing_account_id"),
                        deployments,
                    },
                );
            }
        }

        Ok(projects_map.values().cloned().collect())
    }

    pub async fn execute_with_deps<C>(
        &self,
        deps: &C,
    ) -> Result<Vec<ProjectWithDeployments>, AppError>
    where
        C: HasDbRouter + ?Sized,
    {
        self.execute_with_db(deps.reader_pool(self.consistency))
            .await
    }
}
