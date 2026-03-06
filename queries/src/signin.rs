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

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<SignIn, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .fetch_one(&mut *conn)
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

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<SessionContext, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .fetch_optional(&mut *conn)
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

    pub async fn execute_with<'a, A>(&self, acquirer: A) -> Result<SessionWithSignIns, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let session_row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, active_signin_id
            FROM sessions
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            self.session_id
        )
        .fetch_one(&mut *conn)
        .await?;

        let session = Session {
            id: session_row.id,
            created_at: session_row.created_at,
            updated_at: session_row.updated_at,
            active_signin_id: session_row.active_signin_id,
        };

        let signin_rows = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, session_id, user_id,
                   active_organization_membership_id, active_workspace_membership_id,
                   expires_at, last_active_at, ip_address, browser, device,
                   city, region, region_code, country, country_code
            FROM signins
            WHERE session_id = $1 AND deleted_at IS NULL
            "#,
            self.session_id
        )
        .fetch_all(&mut *conn)
        .await?;

        let signins: Vec<SignIn> = signin_rows
            .into_iter()
            .map(|row| SignIn {
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
            })
            .collect();

        Ok(SessionWithSignIns { session, signins })
    }
}
