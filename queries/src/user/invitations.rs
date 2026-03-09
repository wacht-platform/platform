use super::*;

pub struct DeploymentInvitationQuery {
    deployment_id: i64,
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
    search: Option<String>,
}

impl DeploymentInvitationQuery {
    pub fn new(id: i64) -> Self {
        Self {
            offset: 0,
            sort_key: None,
            sort_order: None,
            limit: 10,
            deployment_id: id,
            search: None,
        }
    }

    pub fn offset(self, offset: i64) -> Self {
        Self { offset, ..self }
    }

    pub fn limit(self, limit: i32) -> Self {
        Self { limit, ..self }
    }

    pub fn sort_key(self, sort_key: Option<String>) -> Self {
        Self { sort_key, ..self }
    }

    pub fn sort_order(self, sort_order: Option<String>) -> Self {
        Self { sort_order, ..self }
    }

    pub fn search(self, search: Option<String>) -> Self {
        Self { search, ..self }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<DeploymentInvitation>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"
            SELECT
                i.id, i.created_at, i.updated_at,
                i.first_name, i.last_name,
                i.email_address, i.deployment_id,
                i.token, i.expiry
            FROM deployment_invitations i
            WHERE i.deployment_id = "#,
        );

        query_builder.push_bind(self.deployment_id);

        if let Some(search_term) = &self.search {
            let trimmed_search = search_term.trim();
            if !trimmed_search.is_empty() {
                let search_pattern = format!("%{}%", trimmed_search);
                query_builder.push(" AND (");
                query_builder.push("i.first_name ILIKE ");
                query_builder.push_bind(search_pattern.clone());
                query_builder.push(" OR i.last_name ILIKE ");
                query_builder.push_bind(search_pattern.clone());
                query_builder.push(" OR i.email_address ILIKE ");
                query_builder.push_bind(search_pattern);
                query_builder.push(")");
            }
        }

        query_builder.push(" ORDER BY ");

        match sort_key {
            "created_at" => query_builder.push("i.created_at"),
            "email" => query_builder.push("i.email_address"),
            _ => query_builder.push("i.created_at"),
        };

        match sort_order.to_lowercase().as_str() {
            "asc" => query_builder.push(" ASC"),
            _ => query_builder.push(" DESC"),
        };

        query_builder.push(" OFFSET ");
        query_builder.push_bind(self.offset);
        query_builder.push(" LIMIT ");
        query_builder.push_bind(self.limit);

        let rows = query_builder.build().fetch_all(executor).await?;

        let invitations = rows
            .into_iter()
            .map(|row| DeploymentInvitation {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                first_name: row.get("first_name"),
                last_name: row.get("last_name"),
                deployment_id: row.get("deployment_id"),
                email_address: row.get("email_address"),
                token: row.get("token"),
                expiry: row.get("expiry"),
            })
            .collect();

        Ok(invitations)
    }
}
