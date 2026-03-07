use chrono::{DateTime, Utc};
use common::error::AppError;
use models::{Session, SignIn};
use serde::{Deserialize, Serialize};
use sqlx::Row;

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

        let signin = SignIn {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            session_id: row.session_id.unwrap_or(0),
            user_id: row.user_id,
            active_organization_membership_id: row.active_organization_membership_id,
            active_workspace_membership_id: row.active_workspace_membership_id,
            expires_at: DateTime::from_naive_utc_and_offset(row.expires_at, Utc),
            last_active_at: DateTime::from_naive_utc_and_offset(row.last_active_at, Utc),
            ip_address: row.ip_address.unwrap_or_default(),
            browser: row.browser.unwrap_or_default(),
            device: row.device.unwrap_or_default(),
            city: row.city.unwrap_or_default(),
            region: row.region.unwrap_or_default(),
            region_code: row.region_code.unwrap_or_default(),
            country: row.country.unwrap_or_default(),
            country_code: row.country_code.unwrap_or_default(),
        };

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

        let user_id: Option<i64> = row.try_get("user_id").ok();
        let user_id = user_id.unwrap_or(0);
        if user_id == 0 {
            return Err(AppError::NotFound("No active user for session".to_string()));
        }

        let active_organization_id: Option<i64> = row.try_get("organization_id").ok().flatten();
        let active_workspace_id: Option<i64> = row.try_get("workspace_id").ok().flatten();

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
                si.session_id,
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
            .filter_map(|row| {
                let signin_id: Option<i64> = row.try_get("signin_id").ok();
                signin_id.map(|id| SignIn {
                    id,
                    created_at: row.get("signin_created_at"),
                    updated_at: row.get("signin_updated_at"),
                    session_id: row.get::<Option<i64>, _>("session_id").unwrap_or(0),
                    user_id: row.get("user_id"),
                    active_organization_membership_id: row.get("active_organization_membership_id"),
                    active_workspace_membership_id: row.get("active_workspace_membership_id"),
                    expires_at: DateTime::from_naive_utc_and_offset(row.get("expires_at"), Utc),
                    last_active_at: DateTime::from_naive_utc_and_offset(
                        row.get("last_active_at"),
                        Utc,
                    ),
                    ip_address: row
                        .get::<Option<String>, _>("ip_address")
                        .unwrap_or_default(),
                    browser: row.get::<Option<String>, _>("browser").unwrap_or_default(),
                    device: row.get::<Option<String>, _>("device").unwrap_or_default(),
                    city: row.get::<Option<String>, _>("city").unwrap_or_default(),
                    region: row.get::<Option<String>, _>("region").unwrap_or_default(),
                    region_code: row
                        .get::<Option<String>, _>("region_code")
                        .unwrap_or_default(),
                    country: row.get::<Option<String>, _>("country").unwrap_or_default(),
                    country_code: row
                        .get::<Option<String>, _>("country_code")
                        .unwrap_or_default(),
                })
            })
            .collect();

        Ok(SessionWithSignIns { session, signins })
    }
}
