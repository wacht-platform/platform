use crate::Query;
use common::state::AppState;
use models::error::AppError;
use models::{AnalyzedEntity, Segment};
use sqlx::QueryBuilder;

pub struct GetSegmentsQuery {
    pub deployment_id: i64,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    pub search: Option<String>,
    pub sort_key: Option<String>,
    pub sort_order: Option<String>,
}

impl Query for GetSegmentsQuery {
    type Output = Vec<Segment>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let limit = self.limit.unwrap_or(20).max(1).min(101);
        let offset = self.offset.unwrap_or(0).max(0);

        let mut query_builder = QueryBuilder::new("SELECT * FROM segments WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);
        query_builder.push(" AND deleted_at IS NULL");

        if let Some(search) = &self.search {
            if !search.is_empty() {
                query_builder.push(" AND name ILIKE ");
                query_builder.push_bind(format!("%{}%", search));
            }
        }

        if let Some(sort_key) = &self.sort_key {
            let order = if self.sort_order.as_deref() == Some("desc") {
                "DESC"
            } else {
                "ASC"
            };

            match sort_key.as_str() {
                "name" => query_builder.push(format!(" ORDER BY name {}", order)),
                "created_at" => query_builder.push(format!(" ORDER BY created_at {}", order)),
                "type" => query_builder.push(format!(" ORDER BY type {}", order)),
                _ => query_builder.push(" ORDER BY created_at DESC"),
            };
        } else {
            query_builder.push(" ORDER BY created_at DESC");
        }

        query_builder.push(" LIMIT ");
        query_builder.push_bind(limit);
        query_builder.push(" OFFSET ");
        query_builder.push_bind(offset);

        let segments = query_builder
            .build_query_as::<Segment>()
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

        Ok(segments)
    }
}

pub struct UserFilter {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
}

pub struct OrganizationFilter {
    pub name: Option<String>,
}

pub struct WorkspaceFilter {
    pub name: Option<String>,
}

pub struct GetSegmentDataQuery {
    pub deployment_id: i64,
    pub target_type: String,
    pub user_filter: Option<UserFilter>,
    pub organization_filter: Option<OrganizationFilter>,
    pub workspace_filter: Option<WorkspaceFilter>,
}

impl Query for GetSegmentDataQuery {
    type Output = Vec<AnalyzedEntity>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let (table, select_clause) = match self.target_type.as_str() {
            "organization" => (
                "organizations",
                "t.id, t.name, NULL::text as first_name, NULL::text as last_name",
            ),
            "workspace" => (
                "workspaces",
                "t.id, t.name, NULL::text as first_name, NULL::text as last_name",
            ),
            "user" => (
                "users",
                "t.id, NULL::text as name, t.first_name, t.last_name",
            ),
            _ => return Err(AppError::BadRequest("Invalid target type".into())),
        };

        let mut query_builder =
            QueryBuilder::new(format!("SELECT {} FROM {} t ", select_clause, table));

        if self.target_type == "user" {
            if let Some(user_filters) = &self.user_filter {
                if user_filters.email.is_some() {
                    query_builder
                        .push(" LEFT JOIN user_email_addresses uea ON t.id = uea.user_id ");
                }
                if user_filters.phone.is_some() {
                    query_builder.push(" LEFT JOIN user_phone_numbers upn ON t.id = upn.user_id ");
                }
            }
        }

        query_builder.push(" WHERE t.deployment_id = ");
        query_builder.push_bind(self.deployment_id);
        query_builder.push(" AND t.deleted_at IS NULL ");

        match self.target_type.as_str() {
            "organization" => {
                if let Some(org_filters) = &self.organization_filter {
                    if let Some(name_val) = &org_filters.name {
                        if !name_val.is_empty() {
                            query_builder.push(" AND t.name ILIKE ");
                            query_builder.push_bind(format!("%{}%", name_val));
                        }
                    }
                }
            }
            "workspace" => {
                if let Some(ws_filters) = &self.workspace_filter {
                    if let Some(name_val) = &ws_filters.name {
                        if !name_val.is_empty() {
                            query_builder.push(" AND t.name ILIKE ");
                            query_builder.push_bind(format!("%{}%", name_val));
                        }
                    }
                }
            }
            "user" => {
                if let Some(user_filters) = &self.user_filter {
                    if let Some(name_val) = &user_filters.name {
                        if !name_val.is_empty() {
                            query_builder.push(" AND (t.first_name ILIKE ");
                            query_builder.push_bind(format!("%{}%", name_val));
                            query_builder.push(" OR t.last_name ILIKE ");
                            query_builder.push_bind(format!("%{}%", name_val));
                            query_builder.push(")");
                        }
                    }
                    if let Some(email_val) = &user_filters.email {
                        if !email_val.is_empty() {
                            query_builder.push(" AND uea.email_address ILIKE ");
                            query_builder.push_bind(format!("%{}%", email_val));
                        }
                    }
                    if let Some(phone_val) = &user_filters.phone {
                        if !phone_val.is_empty() {
                            query_builder.push(" AND upn.phone_number ILIKE ");
                            query_builder.push_bind(format!("%{}%", phone_val));
                        }
                    }
                }
            }
            _ => {}
        }

        query_builder.push(" GROUP BY t.id, t.created_at ORDER BY t.created_at DESC LIMIT 100");

        let entities = query_builder
            .build_query_as::<AnalyzedEntity>()
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

        Ok(entities)
    }
}
