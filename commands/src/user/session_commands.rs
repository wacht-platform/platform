use common::error::AppError;
use sqlx::PgPool;

pub struct RevokeUserSigninCommand {
    deployment_id: i64,
    user_id: i64,
    signin_id: i64,
}

impl RevokeUserSigninCommand {
    pub fn new(deployment_id: i64, user_id: i64, signin_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            signin_id,
        }
    }

    pub async fn execute_with_db(self, pool: &PgPool) -> Result<(), AppError> {
        let row = sqlx::query!(
            r#"
            WITH revoked AS (
                UPDATE signins
                SET deleted_at = NOW(), updated_at = NOW()
                FROM users u
                WHERE signins.user_id = u.id
                  AND u.deployment_id = $1
                  AND signins.user_id = $2
                  AND signins.id = $3
                  AND signins.deleted_at IS NULL
                RETURNING signins.id
            ),
            session_clear AS (
                UPDATE sessions
                SET active_signin_id = NULL, updated_at = NOW()
                WHERE active_signin_id IN (SELECT id FROM revoked)
                  AND deployment_id = $1
            )
            SELECT EXISTS(SELECT 1 FROM revoked) AS "revoked!"
            "#,
            self.deployment_id,
            self.user_id,
            self.signin_id
        )
        .fetch_one(pool)
        .await?;

        if !row.revoked {
            return Err(AppError::NotFound("signin not found".to_string()));
        }
        Ok(())
    }
}

pub struct RevokeAllUserSigninsCommand {
    deployment_id: i64,
    user_id: i64,
}

impl RevokeAllUserSigninsCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }

    pub async fn execute_with_db(self, pool: &PgPool) -> Result<u64, AppError> {
        let row = sqlx::query!(
            r#"
            WITH revoked AS (
                UPDATE signins
                SET deleted_at = NOW(), updated_at = NOW()
                FROM users u
                WHERE signins.user_id = u.id
                  AND u.deployment_id = $1
                  AND signins.user_id = $2
                  AND signins.deleted_at IS NULL
                RETURNING signins.id
            ),
            session_clear AS (
                UPDATE sessions
                SET active_signin_id = NULL, updated_at = NOW()
                WHERE active_signin_id IN (SELECT id FROM revoked)
                  AND deployment_id = $1
            )
            SELECT COUNT(*) AS "revoked_count!" FROM revoked
            "#,
            self.deployment_id,
            self.user_id
        )
        .fetch_one(pool)
        .await?;

        Ok(row.revoked_count as u64)
    }
}
