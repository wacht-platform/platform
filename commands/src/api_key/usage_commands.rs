use crate::Command;
use common::error::AppError;
use common::state::AppState;

pub struct UpdateApiKeyLastUsedCommand {
    pub key_id: i64,
}

impl Command for UpdateApiKeyLastUsedCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl UpdateApiKeyLastUsedCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        sqlx::query!(
            r#"
            UPDATE api_keys
            SET last_used_at = NOW()
            WHERE id = $1
            "#,
            self.key_id
        )
        .execute(&mut *conn)
        .await?;

        Ok(())
    }
}
