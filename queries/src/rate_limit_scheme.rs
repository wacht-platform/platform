use chrono::{DateTime, NaiveDateTime, Utc};
use common::error::AppError;
use models::api_key::RateLimit;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitSchemeData {
    pub id: i64,
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub rules: Vec<RateLimit>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
struct RateLimitSchemeRow {
    id: i64,
    deployment_id: i64,
    slug: String,
    name: String,
    description: Option<String>,
    rules: serde_json::Value,
    created_at: Option<NaiveDateTime>,
    updated_at: Option<NaiveDateTime>,
}

impl From<RateLimitSchemeRow> for RateLimitSchemeData {
    fn from(row: RateLimitSchemeRow) -> Self {
        Self {
            id: row.id,
            deployment_id: row.deployment_id,
            slug: row.slug,
            name: row.name,
            description: row.description,
            rules: serde_json::from_value(row.rules).unwrap_or_else(|_| vec![]),
            created_at: row
                .created_at
                .map(|value| DateTime::from_naive_utc_and_offset(value, Utc))
                .unwrap_or_else(Utc::now),
            updated_at: row
                .updated_at
                .map(|value| DateTime::from_naive_utc_and_offset(value, Utc))
                .unwrap_or_else(Utc::now),
        }
    }
}

pub struct ListRateLimitSchemesQuery {
    pub deployment_id: i64,
}

impl ListRateLimitSchemesQuery {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<RateLimitSchemeData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query_as::<_, RateLimitSchemeRow>(
            r#"
            SELECT
                id,
                deployment_id,
                slug,
                name,
                description,
                rules,
                created_at,
                updated_at
            FROM rate_limit_schemes
            WHERE deployment_id = $1
            ORDER BY updated_at DESC, created_at DESC
            "#,
        )
        .bind(self.deployment_id)
        .fetch_all(executor)
        .await?;

        Ok(rows.into_iter().map(Into::into).collect())
    }
}

pub struct GetRateLimitSchemeQuery {
    pub deployment_id: i64,
    pub slug: String,
}

impl GetRateLimitSchemeQuery {
    pub fn new(deployment_id: i64, slug: String) -> Self {
        Self {
            deployment_id,
            slug,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Option<RateLimitSchemeData>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rec = sqlx::query_as::<_, RateLimitSchemeRow>(
            r#"
            SELECT
                id,
                deployment_id,
                slug,
                name,
                description,
                rules,
                created_at,
                updated_at
            FROM rate_limit_schemes
            WHERE deployment_id = $1
                AND slug = $2
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.slug)
        .fetch_optional(executor)
        .await?;

        Ok(rec.map(Into::into))
    }
}
