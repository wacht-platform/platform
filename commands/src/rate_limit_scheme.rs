use common::error::AppError;
use models::api_key::RateLimit;
use queries::rate_limit_scheme::RateLimitSchemeData;
use sqlx::Row;

fn validate_rules(rules: &[RateLimit]) -> Result<(), AppError> {
    if rules.is_empty() {
        return Err(AppError::Validation(
            "Rate limit scheme must include at least one rule".to_string(),
        ));
    }

    for rule in rules {
        rule.validate().map_err(AppError::Validation)?;
    }

    Ok(())
}

fn map_scheme_row(row: sqlx::postgres::PgRow) -> Result<RateLimitSchemeData, AppError> {
    let rules: serde_json::Value = row.try_get("rules")?;
    Ok(RateLimitSchemeData {
        id: row.try_get("id")?,
        deployment_id: row.try_get("deployment_id")?,
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        rules: serde_json::from_value(rules).unwrap_or_default(),
        created_at: row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("created_at")?
            .unwrap_or_else(chrono::Utc::now),
        updated_at: row
            .try_get::<Option<chrono::DateTime<chrono::Utc>>, _>("updated_at")?
            .unwrap_or_else(chrono::Utc::now),
    })
}

async fn get_scheme_by_slug(
    conn: &mut sqlx::PgConnection,
    deployment_id: i64,
    slug: &str,
) -> Result<Option<RateLimitSchemeData>, AppError> {
    let row = sqlx::query(
        r#"
        SELECT id, deployment_id, slug, name, description, rules, created_at, updated_at
        FROM rate_limit_schemes
        WHERE deployment_id = $1 AND slug = $2
        "#,
    )
    .bind(deployment_id)
    .bind(slug)
    .fetch_optional(&mut *conn)
    .await?;

    row.map(map_scheme_row).transpose()
}

pub struct CreateRateLimitSchemeCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub rules: Vec<RateLimit>,
}

impl CreateRateLimitSchemeCommand {
    pub fn new(
        id: i64,
        deployment_id: i64,
        slug: String,
        name: String,
        rules: Vec<RateLimit>,
    ) -> Self {
        Self {
            id,
            deployment_id,
            slug,
            name,
            description: None,
            rules,
        }
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }
}

impl CreateRateLimitSchemeCommand {
    pub async fn execute_with_db(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
    ) -> Result<RateLimitSchemeData, AppError> {
        let mut conn = acquirer.acquire().await?;
        if self.slug.trim().is_empty() {
            return Err(AppError::Validation("Scheme slug is required".to_string()));
        }
        if self.name.trim().is_empty() {
            return Err(AppError::Validation("Scheme name is required".to_string()));
        }
        validate_rules(&self.rules)?;

        let existing = get_scheme_by_slug(&mut conn, self.deployment_id, &self.slug).await?;
        if existing.is_some() {
            return Err(AppError::Conflict(format!(
                "Rate limit scheme '{}' already exists",
                self.slug
            )));
        }

        let rules_json = serde_json::to_value(&self.rules)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO rate_limit_schemes (id, deployment_id, slug, name, description, rules)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(self.id)
        .bind(self.deployment_id)
        .bind(&self.slug)
        .bind(&self.name)
        .bind(self.description)
        .bind(rules_json)
        .execute(&mut *conn)
        .await?;

        get_scheme_by_slug(&mut conn, self.deployment_id, &self.slug)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to fetch created scheme".to_string()))
    }
}

pub struct UpdateRateLimitSchemeCommand {
    pub deployment_id: i64,
    pub slug: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub rules: Option<Vec<RateLimit>>,
}

impl UpdateRateLimitSchemeCommand {
    pub fn new(deployment_id: i64, slug: String) -> Self {
        Self {
            deployment_id,
            slug,
            name: None,
            description: None,
            rules: None,
        }
    }

    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_rules(mut self, rules: Option<Vec<RateLimit>>) -> Self {
        self.rules = rules;
        self
    }
}

impl UpdateRateLimitSchemeCommand {
    pub async fn execute_with_db(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
    ) -> Result<RateLimitSchemeData, AppError> {
        let mut conn = acquirer.acquire().await?;
        if let Some(name) = &self.name
            && name.trim().is_empty()
        {
            return Err(AppError::Validation(
                "Scheme name cannot be empty".to_string(),
            ));
        }
        if let Some(rules) = &self.rules {
            validate_rules(rules)?;
        }

        let existing = get_scheme_by_slug(&mut conn, self.deployment_id, &self.slug)
            .await?
            .ok_or_else(|| AppError::NotFound("Rate limit scheme not found".to_string()))?;

        let rules_to_store = self.rules.unwrap_or(existing.rules);
        let rules_json = serde_json::to_value(&rules_to_store)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            UPDATE rate_limit_schemes
            SET name = COALESCE($3, name),
                description = COALESCE($4, description),
                rules = $5,
                updated_at = NOW()
            WHERE deployment_id = $1 AND slug = $2
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.slug)
        .bind(self.name)
        .bind(self.description)
        .bind(rules_json)
        .execute(&mut *conn)
        .await?;

        get_scheme_by_slug(&mut conn, self.deployment_id, &self.slug)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to fetch updated scheme".to_string()))
    }
}

pub struct DeleteRateLimitSchemeCommand {
    pub deployment_id: i64,
    pub slug: String,
}

impl DeleteRateLimitSchemeCommand {
    pub fn new(deployment_id: i64, slug: String) -> Self {
        Self { deployment_id, slug }
    }
}

impl DeleteRateLimitSchemeCommand {
    pub async fn execute_with_db(
        self,
        acquirer: impl for<'a> sqlx::Acquire<'a, Database = sqlx::Postgres>,
    ) -> Result<(), AppError> {
        let mut conn = acquirer.acquire().await?;
        let scheme = get_scheme_by_slug(&mut conn, self.deployment_id, &self.slug).await?;
        if scheme.is_none() {
            return Err(AppError::NotFound(
                "Rate limit scheme not found".to_string(),
            ));
        }

        let app_ref_count: i64 = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM api_auth_apps
            WHERE deployment_id = $1
              AND deleted_at IS NULL
              AND rate_limit_scheme_slug = $2
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.slug)
        .fetch_one(&mut *conn)
        .await?
        .try_get("count")?;

        if app_ref_count > 0 {
            return Err(AppError::BadRequest(
                "Cannot delete rate limit scheme while it is assigned to API auth apps".to_string(),
            ));
        }

        let key_ref_count: i64 = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM api_keys
            WHERE deployment_id = $1
              AND revoked_at IS NULL
              AND rate_limit_scheme_slug = $2
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.slug)
        .fetch_one(&mut *conn)
        .await?
        .try_get("count")?;

        if key_ref_count > 0 {
            return Err(AppError::BadRequest(
                "Cannot delete rate limit scheme while it is assigned to active API keys"
                    .to_string(),
            ));
        }

        sqlx::query(
            r#"
            DELETE FROM rate_limit_schemes
            WHERE deployment_id = $1 AND slug = $2
            "#,
        )
        .bind(self.deployment_id)
        .bind(&self.slug)
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
