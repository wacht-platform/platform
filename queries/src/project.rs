use std::collections::BTreeMap;

use sqlx::{Row, query};

use common::error::AppError;
use common::state::AppState;
use models::{Deployment, ProjectWithDeployments};

use super::Query;

#[allow(dead_code)]
pub struct GetProjectsWithDeploymentQuery {
    oid: i64,
    owner_ids: Vec<String>,
}

impl GetProjectsWithDeploymentQuery {
    pub fn new(oid: i64) -> Self {
        GetProjectsWithDeploymentQuery { 
            oid,
            owner_ids: Vec::new(),
        }
    }
    
    pub fn for_owner(owner_id: String) -> Self {
        GetProjectsWithDeploymentQuery {
            oid: 0,
            owner_ids: vec![owner_id],
        }
    }
    
    pub fn for_owners(owner_ids: Vec<String>) -> Self {
        GetProjectsWithDeploymentQuery {
            oid: 0,
            owner_ids,
        }
    }
    
    pub fn for_user_and_organization(user_id: String, org_id: Option<String>) -> Self {
        let mut owner_ids = vec![user_id];
        if let Some(org) = org_id {
            owner_ids.push(org);
        }
        GetProjectsWithDeploymentQuery {
            oid: 0,
            owner_ids,
        }
    }
}

impl GetProjectsWithDeploymentQuery {
    fn create_deployment_from_row(row: &sqlx::postgres::PgRow) -> Deployment {
        Deployment {
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
                .and_then(|v| serde_json::from_value(v).ok()),
            email_verification_records: row
                .get::<Option<serde_json::Value>, _>("deployment_email_verification_records")
                .and_then(|v| serde_json::from_value(v).ok()),
        }
    }
}
impl Query for GetProjectsWithDeploymentQuery {
    type Output = Vec<ProjectWithDeployments>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_str = r#"
            SELECT
                p.id, p.created_at, p.updated_at, p.name, p.image_url,
                p.owner_id,
                d.id as deployment_id, d.created_at as deployment_created_at,
                d.updated_at as deployment_updated_at,
                d.maintenance_mode as deployment_maintenance_mode, d.backend_host as deployment_backend_host,
                d.frontend_host as deployment_frontend_host,
                d.publishable_key as deployment_publishable_key,
                d.project_id as deployment_project_id, d.mode as deployment_mode,
                d.mail_from_host as deployment_mail_from_host,
                d.domain_verification_records::jsonb as deployment_domain_verification_records,
                d.email_verification_records::jsonb as deployment_email_verification_records
            FROM projects p
            LEFT JOIN deployments d ON p.id = d.project_id AND d.deleted_at IS NULL
        "#.to_string();
        
        // Add ownership filtering
        if !self.owner_ids.is_empty() {
            let owner_conditions: Vec<String> = self.owner_ids
                .iter()
                .map(|id| format!("'{}'", id.replace("'", "''")))
                .collect();
            
            query_str.push_str(&format!(" WHERE p.owner_id IN ({})", owner_conditions.join(", ")));
        }
        
        query_str.push_str(" ORDER BY p.id DESC");
        
        let rows = query(&query_str)
        .fetch_all(&app_state.db_pool)
        .await?;

        let mut projects_map: BTreeMap<i64, ProjectWithDeployments> = BTreeMap::new();

        for row in rows {
            let project_id = row.get("id");

            if let Some(project) = projects_map.get_mut(&project_id) {
                if row.get::<Option<i64>, _>("deployment_id").is_some() {
                    project
                        .deployments
                        .push(Self::create_deployment_from_row(&row));
                }
            } else {
                let mut deployments = Vec::new();
                if row.get::<Option<i64>, _>("deployment_id").is_some() {
                    deployments.push(Self::create_deployment_from_row(&row));
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
                        deployments,
                    },
                );
            }
        }

        Ok(projects_map.values().cloned().collect())
    }
}
