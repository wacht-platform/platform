use super::*;
use chrono::{DateTime, Utc};
use models::SignIn;
use sqlx::{Postgres, QueryBuilder};

pub struct GetUserSigninsQuery {
    deployment_id: i64,
    user_id: i64,
    include_expired: bool,
}

impl GetUserSigninsQuery {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            include_expired: false,
        }
    }

    pub fn include_expired(mut self, include_expired: bool) -> Self {
        self.include_expired = include_expired;
        self
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<Vec<SignIn>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"
            SELECT si.id, si.created_at, si.updated_at, si.session_id, si.user_id,
                   si.active_organization_membership_id, si.active_workspace_membership_id,
                   si.expires_at, si.last_active_at, si.ip_address, si.browser,
                   si.device, si.city, si.region, si.region_code, si.country,
                   si.country_code
            FROM signins si
            JOIN users u ON si.user_id = u.id AND u.deployment_id =
            "#,
        );
        qb.push_bind(self.deployment_id);
        qb.push(" WHERE si.deleted_at IS NULL AND si.user_id = ");
        qb.push_bind(self.user_id);
        if !self.include_expired {
            qb.push(" AND si.expires_at > NOW()");
        }
        qb.push(" ORDER BY si.last_active_at DESC");

        let rows = qb.build().fetch_all(executor).await?;
        let signins = rows
            .into_iter()
            .map(|row| {
                let expires_at: chrono::NaiveDateTime = row.get("expires_at");
                let last_active_at: chrono::NaiveDateTime = row.get("last_active_at");
                let user_id: Option<i64> = row.get("user_id");
                let session_id: Option<i64> = row.get("session_id");
                SignIn {
                    id: row.get("id"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                    session_id: session_id.unwrap_or(0),
                    user_id,
                    active_organization_membership_id: row.get("active_organization_membership_id"),
                    active_workspace_membership_id: row.get("active_workspace_membership_id"),
                    expires_at: DateTime::from_naive_utc_and_offset(expires_at, Utc),
                    last_active_at: DateTime::from_naive_utc_and_offset(last_active_at, Utc),
                    ip_address: row.get::<Option<String>, _>("ip_address").unwrap_or_default(),
                    browser: row.get::<Option<String>, _>("browser").unwrap_or_default(),
                    device: row.get::<Option<String>, _>("device").unwrap_or_default(),
                    city: row.get::<Option<String>, _>("city").unwrap_or_default(),
                    region: row.get::<Option<String>, _>("region").unwrap_or_default(),
                    region_code: row.get::<Option<String>, _>("region_code").unwrap_or_default(),
                    country: row.get::<Option<String>, _>("country").unwrap_or_default(),
                    country_code: row
                        .get::<Option<String>, _>("country_code")
                        .unwrap_or_default(),
                }
            })
            .collect();
        Ok(signins)
    }
}
