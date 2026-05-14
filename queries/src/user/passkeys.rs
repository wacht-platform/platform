use super::*;
use models::UserPasskey;
use sqlx::{Postgres, QueryBuilder};

pub struct GetUserPasskeysQuery {
    deployment_id: i64,
    user_id: i64,
}

impl GetUserPasskeysQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<UserPasskey>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT p.id, p.created_at, p.updated_at, p.user_id, p.name,
                   p.sign_count, p.transports, p.last_used_at, p.backed_up,
                   p.device_type
            FROM user_passkeys p
            JOIN users u ON p.user_id = u.id AND u.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(" WHERE p.deleted_at IS NULL AND p.user_id = ");
        qb.push_bind(self.user_id);
        qb.push(" ORDER BY p.created_at DESC");

        let rows = qb.build().fetch_all(executor).await?;
        let passkeys = rows
            .into_iter()
            .map(|row| UserPasskey {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                user_id: row.get("user_id"),
                name: row.get("name"),
                sign_count: row.get("sign_count"),
                transports: row.get("transports"),
                last_used_at: row.get("last_used_at"),
                backed_up: row.get("backed_up"),
                device_type: row.get("device_type"),
            })
            .collect();
        Ok(passkeys)
    }
}
