use chrono::{DateTime, Utc};
use common::error::AppError;
use models::{Session, SignIn};
use serde::{Deserialize, Serialize};
use sqlx::Row;

fn normalize_signin(
    id: i64,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
    session_id: Option<i64>,
    user_id: Option<i64>,
    active_organization_membership_id: Option<i64>,
    active_workspace_membership_id: Option<i64>,
    expires_at: chrono::NaiveDateTime,
    last_active_at: chrono::NaiveDateTime,
    ip_address: Option<String>,
    browser: Option<String>,
    device: Option<String>,
    city: Option<String>,
    region: Option<String>,
    region_code: Option<String>,
    country: Option<String>,
    country_code: Option<String>,
) -> SignIn {
    SignIn {
        id,
        created_at,
        updated_at,
        session_id: session_id.unwrap_or(0),
        user_id,
        active_organization_membership_id,
        active_workspace_membership_id,
        expires_at: DateTime::from_naive_utc_and_offset(expires_at, Utc),
        last_active_at: DateTime::from_naive_utc_and_offset(last_active_at, Utc),
        ip_address: ip_address.unwrap_or_default(),
        browser: browser.unwrap_or_default(),
        device: device.unwrap_or_default(),
        city: city.unwrap_or_default(),
        region: region.unwrap_or_default(),
        region_code: region_code.unwrap_or_default(),
        country: country.unwrap_or_default(),
        country_code: country_code.unwrap_or_default(),
    }
}

pub struct GetSignInQuery {
    signin_id: i64,
}

impl GetSignInQuery {
    pub fn new(signin_id: i64) -> Self {
        Self { signin_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<SignIn, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, session_id, user_id,
                   active_organization_membership_id, active_workspace_membership_id,
                   expires_at, last_active_at, ip_address, browser, device,
                   city, region, region_code, country, country_code
            FROM signins
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            self.signin_id
        )
        .fetch_one(executor)
        .await?;

        let signin = normalize_signin(
            row.id,
            row.created_at,
            row.updated_at,
            row.session_id,
            row.user_id,
            row.active_organization_membership_id,
            row.active_workspace_membership_id,
            row.expires_at,
            row.last_active_at,
            row.ip_address,
            row.browser,
            row.device,
            row.city,
            row.region,
            row.region_code,
            row.country,
            row.country_code,
        );

        Ok(signin)
    }
}

pub struct GetSessionWithActiveContextQuery {
    session_id: i64,
}

impl GetSessionWithActiveContextQuery {
    pub fn new(session_id: i64) -> Self {
        Self { session_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<SessionContext, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query(
            r#"
            SELECT
                si.user_id,
                om.organization_id,
                wm.workspace_id
            FROM sessions s
            LEFT JOIN signins si ON s.active_signin_id = si.id
            LEFT JOIN organization_memberships om ON si.active_organization_membership_id = om.id
            LEFT JOIN workspace_memberships wm ON si.active_workspace_membership_id = wm.id
            WHERE s.id = $1 AND s.deleted_at IS NULL
            "#,
        )
        .bind(self.session_id)
        .fetch_optional(executor)
        .await?;

        let Some(row) = row else {
            return Err(AppError::NotFound("Session not found".to_string()));
        };

        let user_id: Option<i64> = row
            .try_get("user_id")
            .map_err(|e| AppError::Internal(format!("Invalid user_id field: {}", e)))?;
        let user_id = user_id
            .filter(|id| *id > 0)
            .ok_or_else(|| AppError::NotFound("No active user for session".to_string()))?;

        let active_organization_id: Option<i64> = row
            .try_get("organization_id")
            .map_err(|e| AppError::Internal(format!("Invalid organization_id field: {}", e)))?;
        let active_workspace_id: Option<i64> = row
            .try_get("workspace_id")
            .map_err(|e| AppError::Internal(format!("Invalid workspace_id field: {}", e)))?;

        Ok(SessionContext {
            user_id,
            active_organization_id,
            active_workspace_id,
        })
    }
}

#[derive(Debug)]
pub struct SessionContext {
    pub user_id: i64,
    pub active_organization_id: Option<i64>,
    pub active_workspace_id: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionWithSignIns {
    #[serde(flatten)]
    pub session: Session,
    pub signins: Vec<SignIn>,
}

pub struct GetSessionWithSignInsQuery {
    session_id: i64,
}

impl GetSessionWithSignInsQuery {
    pub fn new(session_id: i64) -> Self {
        Self { session_id }
    }

    pub async fn execute_with_db<'e, E>(&self, executor: E) -> Result<SessionWithSignIns, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let rows = sqlx::query(
            r#"
            SELECT
                s.id AS session_id,
                s.created_at AS session_created_at,
                s.updated_at AS session_updated_at,
                s.active_signin_id,
                si.id AS signin_id,
                si.created_at AS signin_created_at,
                si.updated_at AS signin_updated_at,
                si.session_id AS signin_session_id,
                si.user_id,
                si.active_organization_membership_id,
                si.active_workspace_membership_id,
                si.expires_at,
                si.last_active_at,
                si.ip_address,
                si.browser,
                si.device,
                si.city,
                si.region,
                si.region_code,
                si.country,
                si.country_code
            FROM sessions s
            LEFT JOIN signins si
              ON si.session_id = s.id
             AND si.deleted_at IS NULL
            WHERE s.id = $1
              AND s.deleted_at IS NULL
            "#,
        )
        .bind(self.session_id)
        .fetch_all(executor)
        .await?;

        let first = rows
            .first()
            .ok_or_else(|| AppError::NotFound("Session not found".to_string()))?;

        let session = Session {
            id: first.get("session_id"),
            created_at: first.get("session_created_at"),
            updated_at: first.get("session_updated_at"),
            active_signin_id: first.get("active_signin_id"),
        };

        let signins: Vec<SignIn> = rows
            .into_iter()
            .map(|row| -> Result<Option<SignIn>, AppError> {
                let signin_id: Option<i64> = row
                    .try_get("signin_id")
                    .map_err(|e| AppError::Internal(format!("Invalid signin_id field: {}", e)))?;

                Ok(signin_id.map(|id| {
                    normalize_signin(
                        id,
                        row.get("signin_created_at"),
                        row.get("signin_updated_at"),
                        row.get::<Option<i64>, _>("signin_session_id"),
                        row.get("user_id"),
                        row.get("active_organization_membership_id"),
                        row.get("active_workspace_membership_id"),
                        row.get("expires_at"),
                        row.get("last_active_at"),
                        row.get("ip_address"),
                        row.get("browser"),
                        row.get("device"),
                        row.get("city"),
                        row.get("region"),
                        row.get("region_code"),
                        row.get("country"),
                        row.get("country_code"),
                    )
                }))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();

        Ok(SessionWithSignIns { session, signins })
    }
}
