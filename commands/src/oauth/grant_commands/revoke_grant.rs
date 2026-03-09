use super::*;

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
