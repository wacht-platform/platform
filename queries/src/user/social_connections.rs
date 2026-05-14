use super::*;
use models::SocialConnection;
use sqlx::{Postgres, QueryBuilder};

pub struct GetUserSocialConnectionsQuery {
    deployment_id: i64,
    user_id: i64,
}

impl GetUserSocialConnectionsQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<SocialConnection>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT sc.id, sc.created_at, sc.updated_at, sc.user_id,
                   sc.user_email_address_id, sc.provider, sc.email_address,
                   sc.access_token, sc.refresh_token
            FROM social_connections sc
            JOIN users u ON sc.user_id = u.id AND u.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(" WHERE sc.user_id = ");
        qb.push_bind(self.user_id);
        qb.push(" ORDER BY sc.created_at DESC");

        let rows = qb.build().fetch_all(executor).await?;
        let connections = rows
            .into_iter()
            .map(|row| -> Result<SocialConnection, AppError> {
                let provider_str: String = row.get("provider");
                let provider = models::SocialConnectionProvider::from_str(&provider_str)
                    .unwrap_or(models::SocialConnectionProvider::GoogleOauth);
                Ok(SocialConnection {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    user_id: row.get("user_id"),
                    user_email_address_id: row.get("user_email_address_id"),
                    provider,
                    email_address: row.get("email_address"),
                    access_token: row
                        .get::<Option<String>, _>("access_token")
                        .unwrap_or_default(),
                    refresh_token: row
                        .get::<Option<String>, _>("refresh_token")
                        .unwrap_or_default(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(connections)
    }
}
