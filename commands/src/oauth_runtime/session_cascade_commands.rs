//! OIDC: end a session and cascade-revoke every refresh + access token tied
//! to it. Single load-bearing path — logout, RT-reuse detection, admin
//! force-sign-out all funnel through this.

use common::error::AppError;
use sqlx::PgPool;

pub struct RevokeSessionAndCascadeTokens {
    pub session_id: i64,
}

pub struct SessionCascadeResult {
    pub sessions_updated: u64,
    pub refresh_tokens_revoked: u64,
    pub access_tokens_revoked: u64,
}

impl RevokeSessionAndCascadeTokens {
    pub async fn execute_with_pool(
        self,
        pool: &PgPool,
    ) -> Result<SessionCascadeResult, AppError> {
        let mut tx = pool.begin().await?;

        let sessions = sqlx::query!(
            r#"
            UPDATE sessions
               SET deleted_at = NOW()
             WHERE id = $1
               AND deleted_at IS NULL
            "#,
            self.session_id
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let refresh = sqlx::query!(
            r#"
            UPDATE oauth_refresh_tokens
               SET revoked_at = NOW()
             WHERE session_id = $1
               AND revoked_at IS NULL
            "#,
            self.session_id
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        let access = sqlx::query!(
            r#"
            UPDATE oauth_access_tokens
               SET revoked_at = NOW()
             WHERE session_id = $1
               AND revoked_at IS NULL
            "#,
            self.session_id
        )
        .execute(&mut *tx)
        .await?
        .rows_affected();

        tx.commit().await?;

        Ok(SessionCascadeResult {
            sessions_updated: sessions,
            refresh_tokens_revoked: refresh,
            access_tokens_revoked: access,
        })
    }
}
