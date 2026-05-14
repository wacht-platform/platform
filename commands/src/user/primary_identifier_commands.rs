use common::error::AppError;
use sqlx::PgPool;

pub struct MakeUserEmailPrimaryCommand {
    deployment_id: i64,
    user_id: i64,
    email_id: i64,
}

impl MakeUserEmailPrimaryCommand {
    pub fn new(deployment_id: i64, user_id: i64, email_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            email_id,
        }
    }

    pub async fn execute_with_pool(self, pool: &PgPool) -> Result<(), AppError> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query!(
            r#"
            SELECT e.verified
            FROM user_email_addresses e
            JOIN users u ON e.user_id = u.id
            WHERE e.id = $1 AND e.user_id = $2 AND u.deployment_id = $3
            "#,
            self.email_id,
            self.user_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let row = row.ok_or_else(|| AppError::NotFound("email not found".to_string()))?;
        if !row.verified {
            return Err(AppError::BadRequest(
                "email must be verified before it can be marked primary".to_string(),
            ));
        }

        sqlx::query!(
            r#"
            UPDATE user_email_addresses
            SET is_primary = false, updated_at = NOW()
            WHERE user_id = $1 AND is_primary = true
            "#,
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE user_email_addresses
            SET is_primary = true, updated_at = NOW()
            WHERE id = $1
            "#,
            self.email_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            r#"
            UPDATE users
            SET primary_email_address_id = $1, updated_at = NOW()
            WHERE id = $2 AND deployment_id = $3
            "#,
            self.email_id,
            self.user_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}

pub struct MakeUserPhonePrimaryCommand {
    deployment_id: i64,
    user_id: i64,
    phone_id: i64,
}

impl MakeUserPhonePrimaryCommand {
    pub fn new(deployment_id: i64, user_id: i64, phone_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            phone_id,
        }
    }

    pub async fn execute_with_pool(self, pool: &PgPool) -> Result<(), AppError> {
        let mut tx = pool.begin().await?;

        let row = sqlx::query!(
            r#"
            SELECT p.verified
            FROM user_phone_numbers p
            JOIN users u ON p.user_id = u.id
            WHERE p.id = $1 AND p.user_id = $2 AND u.deployment_id = $3
            "#,
            self.phone_id,
            self.user_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let row = row.ok_or_else(|| AppError::NotFound("phone not found".to_string()))?;
        if !row.verified {
            return Err(AppError::BadRequest(
                "phone must be verified before it can be marked primary".to_string(),
            ));
        }

        sqlx::query!(
            r#"
            UPDATE users
            SET primary_phone_number_id = $1, updated_at = NOW()
            WHERE id = $2 AND deployment_id = $3
            "#,
            self.phone_id,
            self.user_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }
}
