use super::*;

pub struct GetUserAuthenticatorQuery {
    user_id: i64,
}

impl GetUserAuthenticatorQuery {
    pub fn new(user_id: i64) -> Self {
        Self { user_id }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<models::UserAuthenticator, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, user_id, totp_secret, otp_url
            FROM user_authenticators
            WHERE user_id = $1 AND deleted_at IS NULL
            "#,
            self.user_id
        )
        .fetch_one(executor)
        .await?;

        Ok(models::UserAuthenticator {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            user_id: row.user_id.unwrap_or(0),
            totp_secret: row.totp_secret,
            otp_url: row.otp_url,
        })
    }
}
