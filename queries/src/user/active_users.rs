use super::*;

pub struct DeploymentActiveUserListQuery {
    offset: i64,
    sort_key: Option<String>,
    sort_order: Option<String>,
    limit: i32,
    deployment_id: i64,
    search: Option<String>,
}

impl DeploymentActiveUserListQuery {
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
    ) -> Result<Vec<UserWithIdentifiers>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let sort_key = self.sort_key.as_deref().unwrap_or("created_at");
        let sort_order = self.sort_order.as_deref().unwrap_or("desc");

        let mut query_builder = sqlx::QueryBuilder::new(
            r#"
            SELECT
                u.id, u.created_at, u.updated_at,
                u.first_name, u.last_name, u.username, u.profile_picture_url,
                e.email_address as primary_email_address,
                p.country_code || p.phone_number as primary_phone_number
            FROM users u
            LEFT JOIN user_email_addresses e ON u.primary_email_address_id = e.id
            LEFT JOIN user_phone_numbers p ON u.primary_phone_number_id = p.id
            WHERE u.deployment_id = "#,
        );

        query_builder.push_bind(self.deployment_id);
        query_builder.push(" AND u.deleted_at IS NULL");

        if let Some(search_term) = &self.search {
            let trimmed_search = search_term.trim();
            if !trimmed_search.is_empty() {
                query_builder.push(
                    r#" AND EXISTS (
                        SELECT 1
                        FROM search_users su
                        WHERE su.user_id = u.id
                          AND su.deployment_id = "#,
                );
                query_builder.push_bind(self.deployment_id);
                query_builder.push(
                    r#" AND (
                        su.search_vector @@ websearch_to_tsquery('english', "#,
                );
                query_builder.push_bind(trimmed_search);
                query_builder.push(
                    r#")
                        OR su.first_name % "#,
                );
                query_builder.push_bind(trimmed_search);
                query_builder.push(
                    r#"
                        OR su.last_name % "#,
                );
                query_builder.push_bind(trimmed_search);
                query_builder.push(
                    r#"
                        OR su.username % "#,
                );
                query_builder.push_bind(trimmed_search);
                query_builder.push(
                    r#"
                        OR su.primary_email % "#,
                );
                query_builder.push_bind(trimmed_search);
                query_builder.push(
                    r#"
                    )
                )"#,
                );
            }
        }

        query_builder.push(" ORDER BY ");

        match sort_key {
            "created_at" => query_builder.push("u.created_at"),
            "username" => query_builder.push("u.username"),
            "email" => query_builder.push("e.email_address"),
            "phone_number" => query_builder.push("p.phone_number"),
            _ => query_builder.push("u.created_at"),
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

        let users = rows
            .into_iter()
            .map(|row| UserWithIdentifiers {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                first_name: row.get("first_name"),
                last_name: row.get("last_name"),
                username: row.get("username"),
                profile_picture_url: row.get("profile_picture_url"),
                primary_email_address: row.get("primary_email_address"),
                primary_phone_number: row.get("primary_phone_number"),
            })
            .collect();

        Ok(users)
    }
}
