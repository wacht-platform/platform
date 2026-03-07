use common::error::AppError;

pub struct CleanupRotatingTokenCommand {
    pub rotating_token_id: i64,
}

impl CleanupRotatingTokenCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let res = sqlx::query!(
            r#"
            DELETE FROM rotating_tokens
            WHERE id = $1
              AND next_token_id IS NOT NULL
              AND valid_until < NOW()
            "#,
            self.rotating_token_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(res.rows_affected() > 0)
    }
}

pub struct CleanupOrphanSessionCommand {
    pub session_id: i64,
}

impl CleanupOrphanSessionCommand {
    pub async fn execute_with_db<'a, A>(self, acquirer: A) -> Result<bool, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let res = sqlx::query!(
            r#"
            DELETE FROM sessions s
            WHERE s.id = $1
              AND s.active_signin_id IS NULL
              AND NOT EXISTS (SELECT 1 FROM signins si WHERE si.session_id = s.id)
              AND NOT EXISTS (SELECT 1 FROM sign_in_attempts sia WHERE sia.session_id = s.id)
              AND NOT EXISTS (SELECT 1 FROM signup_attempts sua WHERE sua.session_id = s.id)
              AND NOT EXISTS (SELECT 1 FROM rotating_tokens rt WHERE rt.session_id = s.id)
              AND NOT EXISTS (SELECT 1 FROM agent_sessions ags WHERE ags.session_id = s.id)
              AND NOT EXISTS (SELECT 1 FROM webhook_app_sessions was WHERE was.session_id = s.id)
              AND NOT EXISTS (SELECT 1 FROM api_auth_app_sessions aas WHERE aas.session_id = s.id)
            "#,
            self.session_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(res.rows_affected() > 0)
    }
}
