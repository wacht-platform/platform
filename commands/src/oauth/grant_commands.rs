use crate::Command;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;

pub struct CreateOAuthClientGrantCommand {
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

impl Command for CreateOAuthClientGrantCommand {
    type Output = OAuthClientGrantCreated;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer(), app_state.sf.next_id()? as i64)
            .await
    }
}

impl CreateOAuthClientGrantCommand {
    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        grant_id: i64,
    ) -> Result<OAuthClientGrantCreated, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn, grant_id).await
    }

    async fn execute_with_deps<C>(
        self,
        mut conn: C,
        grant_id: i64,
    ) -> Result<OAuthClientGrantCreated, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
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
            .and_then(|v| v.parse::<i64>().ok())
            .filter(|id| *id > 0)
            .is_some();
        if !valid_resource {
            return Err(AppError::Validation(
                "resource must be an absolute URI (e.g. urn:wacht:workspace:123)".to_string(),
            ));
        }
        let scope_policy = sqlx::query!(
            r#"
            SELECT oa.supported_scopes as "supported_scopes: serde_json::Value"
            FROM oauth_clients c
            INNER JOIN oauth_apps oa
              ON oa.id = c.oauth_app_id
             AND oa.deployment_id = c.deployment_id
            WHERE c.deployment_id = $1
              AND c.id = $2
            "#,
            self.deployment_id,
            self.oauth_client_id
        )
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound("OAuth client not found".to_string()))?;

        let supported_scopes: Vec<String> =
            serde_json::from_value(scope_policy.supported_scopes).unwrap_or_default();
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

        let rec = sqlx::query!(
            r#"
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
            VALUES ($1,$2,$3,$4,$5,$6,'active',NOW(),$7,$8,NOW(),NOW())
            RETURNING id
            "#,
            grant_id,
            self.deployment_id,
            self.api_auth_app_slug,
            self.oauth_client_id,
            resource,
            serde_json::to_value(&self.scopes)?,
            self.expires_at,
            self.granted_by_user_id
        )
        .fetch_one(&mut *conn)
        .await?;

        Ok(OAuthClientGrantCreated { id: rec.id })
    }
}

pub struct RevokeOAuthClientGrantCommand {
    pub deployment_id: i64,
    pub oauth_client_id: i64,
    pub grant_id: i64,
}

impl Command for RevokeOAuthClientGrantCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state.db_router.writer()).await
    }
}

impl RevokeOAuthClientGrantCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<(), AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
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
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
