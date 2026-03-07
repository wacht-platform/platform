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
