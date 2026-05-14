use common::error::AppError;

pub struct RenameUserPasskeyCommand {
    deployment_id: i64,
    user_id: i64,
    passkey_id: i64,
    name: String,
}

impl RenameUserPasskeyCommand {
    pub fn new(deployment_id: i64, user_id: i64, passkey_id: i64, name: String) -> Self {
        Self {
            deployment_id,
            user_id,
            passkey_id,
            name,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            return Err(AppError::BadRequest("name cannot be empty".to_string()));
        }
        let result = sqlx::query!(
            r#"
            UPDATE user_passkeys
            SET name = $1, updated_at = NOW()
            FROM users u
            WHERE user_passkeys.user_id = u.id
              AND u.deployment_id = $2
              AND user_passkeys.user_id = $3
              AND user_passkeys.id = $4
              AND user_passkeys.deleted_at IS NULL
            "#,
            trimmed,
            self.deployment_id,
            self.user_id,
            self.passkey_id
        )
        .execute(executor)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("passkey not found".to_string()));
        }
        Ok(())
    }
}

pub struct DeleteUserPasskeyCommand {
    deployment_id: i64,
    user_id: i64,
    passkey_id: i64,
}

impl DeleteUserPasskeyCommand {
    pub fn new(deployment_id: i64, user_id: i64, passkey_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            passkey_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            UPDATE user_passkeys
            SET deleted_at = NOW(), updated_at = NOW()
            FROM users u
            WHERE user_passkeys.user_id = u.id
              AND u.deployment_id = $1
              AND user_passkeys.user_id = $2
              AND user_passkeys.id = $3
              AND user_passkeys.deleted_at IS NULL
            "#,
            self.deployment_id,
            self.user_id,
            self.passkey_id
        )
        .execute(executor)
        .await?;
        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("passkey not found".to_string()));
        }
        Ok(())
    }
}
