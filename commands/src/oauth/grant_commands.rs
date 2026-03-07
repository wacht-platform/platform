use chrono::{DateTime, Utc};
use common::error::AppError;
use serde::de::DeserializeOwned;

fn json_default<T: DeserializeOwned + Default>(value: serde_json::Value) -> T {
    serde_json::from_value(value).unwrap_or_default()
}

pub struct CreateOAuthClientGrantCommand {
    pub grant_id: Option<i64>,
    pub deployment_id: i64,
    pub api_auth_app_slug: String,
    pub oauth_client_id: i64,
    pub resource: String,
    pub scopes: Vec<String>,
    pub granted_by_user_id: Option<i64>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct OAuthClientGrantCreated {
    pub id: i64,
}

impl CreateOAuthClientGrantCommand {
    pub fn with_grant_id(mut self, grant_id: i64) -> Self {
        self.grant_id = Some(grant_id);
        self
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<OAuthClientGrantCreated, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let grant_id = self
            .grant_id
            .ok_or_else(|| AppError::Validation("grant_id is required".to_string()))?;
        let resource = self.resource.trim();
        if resource.is_empty() {
            return Err(AppError::Validation("resource is required".to_string()));
        }
        if resource == "*" || resource.eq_ignore_ascii_case("all") {
            return Err(AppError::Validation(
                "wildcard/all resource grants are not allowed".to_string(),
            ));
        }
        let valid_resource = resource
            .strip_prefix("urn:wacht:organization:")
            .or_else(|| resource.strip_prefix("urn:wacht:workspace:"))
            .or_else(|| resource.strip_prefix("urn:wacht:user:"))
            .map(|v| {
                v.parse::<i64>().map_err(|_| {
                    AppError::Validation(
                        "resource must be an absolute URI (e.g. urn:wacht:workspace:123)"
                            .to_string(),
                    )
                })
            })
            .transpose()?
            .filter(|id| *id > 0)
            .is_some();
        if !valid_resource {
            return Err(AppError::Validation(
                "resource must be an absolute URI (e.g. urn:wacht:workspace:123)".to_string(),
            ));
        }
        let scopes_json = serde_json::to_value(&self.scopes)?;
        let row = sqlx::query!(
            r#"
            WITH client AS (
                SELECT oa.supported_scopes
                FROM oauth_clients c
                INNER JOIN oauth_apps oa
                  ON oa.id = c.oauth_app_id
                 AND oa.deployment_id = c.deployment_id
                WHERE c.deployment_id = $1
                  AND c.id = $2
            ),
            ins AS (
                INSERT INTO oauth_client_grants (
                    id,
                    deployment_id,
                    app_slug,
                    oauth_client_id,
                    resource,
                    scopes,
                    status,
                    granted_at,
                    expires_at,
                    granted_by_user_id,
                    created_at,
                    updated_at
                )
                SELECT
                    $3,
                    $1,
                    $4,
                    $2,
                    $5,
                    $6,
                    'active',
                    NOW(),
                    $7,
                    $8,
                    NOW(),
                    NOW()
                FROM client
                WHERE jsonb_array_length((SELECT supported_scopes FROM client)) > 0
                  AND NOT EXISTS (
                    SELECT 1
                    FROM jsonb_array_elements_text($6::jsonb) req(scope)
                    WHERE NOT ((SELECT supported_scopes FROM client) ? req.scope)
                  )
                RETURNING id
            )
            SELECT
                EXISTS(SELECT 1 FROM client) AS "client_exists!",
                COALESCE((SELECT supported_scopes FROM client), '[]'::jsonb) AS "supported_scopes!: serde_json::Value",
                (SELECT id FROM ins) AS "grant_id?"
            "#,
            self.deployment_id,
            self.oauth_client_id,
            grant_id,
            self.api_auth_app_slug,
            resource,
            scopes_json,
            self.expires_at,
            self.granted_by_user_id
        )
        .fetch_one(executor)
        .await?;

        if !row.client_exists {
            return Err(AppError::NotFound("OAuth client not found".to_string()));
        }

        let supported_scopes: Vec<String> = json_default(row.supported_scopes);
        if supported_scopes.is_empty() {
            return Err(AppError::Validation(
                "OAuth app has no supported scopes configured".to_string(),
            ));
        }

        let invalid_scopes: Vec<String> = self
            .scopes
            .iter()
            .filter(|scope| !supported_scopes.iter().any(|s| s == *scope))
            .cloned()
            .collect();
        if !invalid_scopes.is_empty() {
            return Err(AppError::Validation(format!(
                "Unsupported scopes for this OAuth app: {}",
                invalid_scopes.join(", ")
            )));
        }
        let inserted_id = row.grant_id.ok_or_else(|| {
            AppError::BadRequest("Failed to create OAuth client grant".to_string())
        })?;

        Ok(OAuthClientGrantCreated { id: inserted_id })
    }
}

pub struct RevokeOAuthClientGrantCommand {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub grant_id: i64,
}

impl RevokeOAuthClientGrantCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            UPDATE oauth_client_grants
            SET
                status = 'revoked',
                revoked_at = NOW(),
                updated_at = NOW()
            WHERE deployment_id = $1
              AND oauth_client_id = $2
              AND id = $3
              AND status = 'active'
            "#,
            self.deployment_id,
            self.oauth_client_id,
            self.grant_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
