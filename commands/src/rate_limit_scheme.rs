use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::api_key::RateLimit;
use queries::{Query, rate_limit_scheme::RateLimitSchemeData};
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

pub struct CreateRateLimitSchemeCommand {
    pub deployment_id: i64,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub rules: Vec<RateLimit>,
}

impl Command for CreateRateLimitSchemeCommand {
    type Output = RateLimitSchemeData;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.slug.trim().is_empty() {
            return Err(AppError::Validation("Scheme slug is required".to_string()));
        }
        if self.name.trim().is_empty() {
            return Err(AppError::Validation("Scheme name is required".to_string()));
        }
        validate_rules(&self.rules)?;

        let existing = queries::rate_limit_scheme::GetRateLimitSchemeQuery::new(
            self.deployment_id,
            self.slug.clone(),
        )
        .execute(app_state)
        .await?;
        if existing.is_some() {
            return Err(AppError::Conflict(format!(
                "Rate limit scheme '{}' already exists",
                self.slug
            )));
        }

        let id = app_state.sf.next_id()? as i64;
        let rules_json = serde_json::to_value(&self.rules)
            .map_err(|e| AppError::Serialization(e.to_string()))?;

        sqlx::query(
            r#"
            INSERT INTO rate_limit_schemes (id, deployment_id, slug, name, description, rules)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(id)
        .bind(self.deployment_id)
        .bind(&self.slug)
        .bind(&self.name)
        .bind(self.description)
        .bind(rules_json)
        .execute(&app_state.db_pool)
        .await?;

        queries::rate_limit_scheme::GetRateLimitSchemeQuery::new(self.deployment_id, self.slug)
            .execute(app_state)
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

impl Command for UpdateRateLimitSchemeCommand {
    type Output = RateLimitSchemeData;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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

        let existing = queries::rate_limit_scheme::GetRateLimitSchemeQuery::new(
            self.deployment_id,
            self.slug.clone(),
        )
        .execute(app_state)
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
        .execute(&app_state.db_pool)
        .await?;

        queries::rate_limit_scheme::GetRateLimitSchemeQuery::new(self.deployment_id, self.slug)
            .execute(app_state)
            .await?
            .ok_or_else(|| AppError::Internal("Failed to fetch updated scheme".to_string()))
    }
}

pub struct DeleteRateLimitSchemeCommand {
    pub deployment_id: i64,
    pub slug: String,
}

impl Command for DeleteRateLimitSchemeCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let scheme = queries::rate_limit_scheme::GetRateLimitSchemeQuery::new(
            self.deployment_id,
            self.slug.clone(),
        )
        .execute(app_state)
        .await?;
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
        .fetch_one(&app_state.db_pool)
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
        .fetch_one(&app_state.db_pool)
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
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}
