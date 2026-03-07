use common::error::AppError;
use models::api_key::RateLimit;
use queries::rate_limit_scheme::RateLimitSchemeData;

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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<RateLimitSchemeData, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let slug = self.slug;
        let name = self.name;
        let description = self.description;

        if slug.trim().is_empty() {
            return Err(AppError::Validation("Scheme slug is required".to_string()));
        }
        if name.trim().is_empty() {
            return Err(AppError::Validation("Scheme name is required".to_string()));
        }
        validate_rules(&self.rules)?;

        let rules_json = serde_json::to_value(&self.rules)
            .map_err(|e| AppError::Serialization(e.to_string()))?;
        let row = sqlx::query!(
            r#"
            WITH existing AS (
                SELECT id
                FROM rate_limit_schemes
                WHERE deployment_id = $2
                  AND slug = $3
            ),
            ins AS (
                INSERT INTO rate_limit_schemes (id, deployment_id, slug, name, description, rules)
                SELECT $1, $2, $3, $4, $5, $6
                WHERE NOT EXISTS(SELECT 1 FROM existing)
                RETURNING id, deployment_id, slug, name, description, rules,
                          created_at,
                          updated_at
            )
            SELECT
                EXISTS(SELECT 1 FROM existing) AS "already_exists!",
                ins.id,
                ins.deployment_id,
                ins.slug,
                ins.name,
                ins.description,
                ins.rules,
                ins.created_at AS "created_at!",
                ins.updated_at AS "updated_at!"
            FROM ins
            UNION ALL
            SELECT
                EXISTS(SELECT 1 FROM existing) AS "already_exists!",
                NULL::BIGINT AS id,
                NULL::BIGINT AS deployment_id,
                NULL::TEXT AS slug,
                NULL::TEXT AS name,
                NULL::TEXT AS description,
                NULL::JSONB AS rules,
                NOW() AS "created_at!",
                NOW() AS "updated_at!"
            WHERE NOT EXISTS(SELECT 1 FROM ins)
            LIMIT 1
            "#,
            self.id,
            self.deployment_id,
            &slug,
            &name,
            description.clone(),
            rules_json
        )
        .fetch_one(executor)
        .await?;

        if row.already_exists {
            return Err(AppError::Conflict(format!(
                "Rate limit scheme '{}' already exists",
                slug
            )));
        }

        Ok(RateLimitSchemeData {
            id: row.id.unwrap_or(self.id),
            deployment_id: row.deployment_id.unwrap_or(self.deployment_id),
            slug: row.slug.unwrap_or(slug),
            name: row.name.unwrap_or(name),
            description: row.description,
            rules: row
                .rules
                .and_then(|v| serde_json::from_value(v).ok())
                .unwrap_or_default(),
            created_at: row.created_at,
            updated_at: row.updated_at,
        })
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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<RateLimitSchemeData, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
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
        let rules_json = match self.rules {
            Some(rules) => Some(
                serde_json::to_value(&rules).map_err(|e| AppError::Serialization(e.to_string()))?,
            ),
            None => None,
        };

        let row = sqlx::query!(
            r#"
            UPDATE rate_limit_schemes
            SET name = COALESCE($3, name),
                description = COALESCE($4, description),
                rules = COALESCE($5, rules),
                updated_at = NOW()
            WHERE deployment_id = $1
              AND slug = $2
            RETURNING
                id,
                deployment_id,
                slug,
                name,
                description,
                rules,
                created_at as "created_at!",
                updated_at as "updated_at!"
            "#,
            self.deployment_id,
            self.slug,
            self.name,
            self.description,
            rules_json
        )
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound("Rate limit scheme not found".to_string()))?;

        Ok(RateLimitSchemeData {
            id: row.id,
            deployment_id: row.deployment_id,
            slug: row.slug,
            name: row.name,
            description: row.description,
            rules: serde_json::from_value(row.rules).unwrap_or_default(),
            created_at: chrono::DateTime::from_naive_utc_and_offset(row.created_at, chrono::Utc),
            updated_at: chrono::DateTime::from_naive_utc_and_offset(row.updated_at, chrono::Utc),
        })
    }
}

pub struct DeleteRateLimitSchemeCommand {
    pub deployment_id: i64,
    pub slug: String,
}

impl DeleteRateLimitSchemeCommand {
    pub fn new(deployment_id: i64, slug: String) -> Self {
        Self {
            deployment_id,
            slug,
        }
    }
}

impl DeleteRateLimitSchemeCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH scheme AS (
                SELECT 1
                FROM rate_limit_schemes
                WHERE deployment_id = $1 AND slug = $2
            ),
            app_refs AS (
                SELECT COUNT(*)::BIGINT AS count
                FROM api_auth_apps
                WHERE deployment_id = $1
                  AND deleted_at IS NULL
                  AND rate_limit_scheme_slug = $2
            ),
            key_refs AS (
                SELECT COUNT(*)::BIGINT AS count
                FROM api_keys
                WHERE deployment_id = $1
                  AND revoked_at IS NULL
                  AND rate_limit_scheme_slug = $2
            ),
            del AS (
                DELETE FROM rate_limit_schemes
                WHERE deployment_id = $1
                  AND slug = $2
                  AND (SELECT count FROM app_refs) = 0
                  AND (SELECT count FROM key_refs) = 0
            )
            SELECT
                EXISTS(SELECT 1 FROM scheme) AS "scheme_exists!",
                (SELECT count FROM app_refs) AS "app_ref_count!",
                (SELECT count FROM key_refs) AS "key_ref_count!"
            "#,
            self.deployment_id,
            self.slug
        )
        .fetch_one(executor)
        .await?;

        if !result.scheme_exists {
            return Err(AppError::NotFound(
                "Rate limit scheme not found".to_string(),
            ));
        }
        if result.app_ref_count > 0 {
            return Err(AppError::BadRequest(
                "Cannot delete rate limit scheme while it is assigned to API auth apps".to_string(),
            ));
        }
        if result.key_ref_count > 0 {
            return Err(AppError::BadRequest(
                "Cannot delete rate limit scheme while it is assigned to active API keys"
                    .to_string(),
            ));
        }

        Ok(())
    }
}
