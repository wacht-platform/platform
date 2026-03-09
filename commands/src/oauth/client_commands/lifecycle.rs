use super::*;
pub struct SetOAuthClientRegistrationAccessToken {
    pub oauth_app_id: i64,
    pub client_id: String,
    pub registration_access_token_hash: Option<String>,
}

impl SetOAuthClientRegistrationAccessToken {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<bool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let res = sqlx::query!(
            r#"
            UPDATE oauth_clients
            SET
                registration_access_token_hash = $3,
                updated_at = NOW()
            WHERE oauth_app_id = $1
              AND client_id = $2
            "#,
            self.oauth_app_id,
            self.client_id,
            self.registration_access_token_hash
        )
        .execute(executor)
        .await?;

        Ok(res.rows_affected() > 0)
    }
}

pub struct DeactivateOAuthClient {
    pub oauth_app_id: i64,
    pub client_id: String,
}

impl DeactivateOAuthClient {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<bool, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let res = sqlx::query!(
            r#"
            UPDATE oauth_clients
            SET
                is_active = FALSE,
                registration_access_token_hash = NULL,
                updated_at = NOW()
            WHERE oauth_app_id = $1
              AND client_id = $2
              AND is_active = TRUE
            "#,
            self.oauth_app_id,
            self.client_id
        )
        .execute(executor)
        .await?;

        Ok(res.rows_affected() > 0)
    }
}
