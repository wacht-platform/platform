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
        self.execute_with(&app_state.db_pool).await
    }
}

impl GetSegmentsQuery {
    pub async fn execute_with(&self, pool: &sqlx::PgPool) -> Result<Vec<Segment>, AppError> {
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
            .fetch_all(pool)
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
    pub segment_id: Option<i64>,
    pub user_filter: Option<UserFilter>,
    pub organization_filter: Option<OrganizationFilter>,
    pub workspace_filter: Option<WorkspaceFilter>,
}

impl Query for GetSegmentDataQuery {
    type Output = Vec<AnalyzedEntity>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl GetSegmentDataQuery {
    pub async fn execute_with(&self, pool: &sqlx::PgPool) -> Result<Vec<AnalyzedEntity>, AppError> {
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

        if self.segment_id.is_some() {
            match self.target_type.as_str() {
                "organization" => {
                    query_builder
                        .push(" INNER JOIN organization_segments os ON t.id = os.organization_id ");
                }
                "workspace" => {
                    query_builder
                        .push(" INNER JOIN workspace_segments ws ON t.id = ws.workspace_id ");
                }
                "user" => {
                    query_builder.push(" INNER JOIN user_segments us ON t.id = us.user_id ");
                }
                _ => {}
            }
        }

        query_builder.push(" WHERE t.deployment_id = ");
        query_builder.push_bind(self.deployment_id);
        query_builder.push(" AND t.deleted_at IS NULL ");

        if let Some(segment_id) = self.segment_id {
            match self.target_type.as_str() {
                "organization" => {
                    query_builder.push(" AND os.segment_id = ");
                }
                "workspace" => {
                    query_builder.push(" AND ws.segment_id = ");
                }
                "user" => {
                    query_builder.push(" AND us.segment_id = ");
                }
                _ => {}
            }
            query_builder.push_bind(segment_id);
        }

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
                    let mut has_any_user_filter = false;

                    if let Some(name_val) = &user_filters.name {
                        if !name_val.trim().is_empty() {
                            if !has_any_user_filter {
                                query_builder.push(
                                    " AND EXISTS (SELECT 1 FROM search_users su WHERE su.user_id = t.id AND su.deployment_id = t.deployment_id ",
                                );
                                has_any_user_filter = true;
                            }
                            query_builder
                                .push(" AND su.search_vector @@ websearch_to_tsquery('english', ");
                            query_builder.push_bind(name_val.trim());
                            query_builder.push(")");
                        }
                    }
                    if let Some(email_val) = &user_filters.email {
                        if !email_val.trim().is_empty() {
                            if !has_any_user_filter {
                                query_builder.push(
                                    " AND EXISTS (SELECT 1 FROM search_users su WHERE su.user_id = t.id AND su.deployment_id = t.deployment_id ",
                                );
                                has_any_user_filter = true;
                            }
                            query_builder
                                .push(" AND su.search_vector @@ websearch_to_tsquery('english', ");
                            query_builder.push_bind(email_val.trim());
                            query_builder.push(")");
                        }
                    }
                    if let Some(phone_val) = &user_filters.phone {
                        if !phone_val.trim().is_empty() {
                            if !has_any_user_filter {
                                query_builder.push(
                                    " AND EXISTS (SELECT 1 FROM search_users su WHERE su.user_id = t.id AND su.deployment_id = t.deployment_id ",
                                );
                                has_any_user_filter = true;
                            }
                            query_builder
                                .push(" AND su.search_vector @@ websearch_to_tsquery('english', ");
                            query_builder.push_bind(phone_val.trim());
                            query_builder.push(")");
                        }
                    }

                    if has_any_user_filter {
                        query_builder.push(")");
                    }
                }
            }
            _ => {}
        }

        query_builder.push(" GROUP BY t.id, t.created_at ORDER BY t.created_at DESC LIMIT 100");

        let entities = query_builder
            .build_query_as::<AnalyzedEntity>()
            .fetch_all(pool)
            .await
            .map_err(AppError::Database)?;

        Ok(entities)
    }
}
