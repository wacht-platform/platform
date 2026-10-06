use super::*;

#[cfg(test)]
mod tests {
    #[test]
    fn authenticator_output_omits_enrollment_secrets() {
        let authenticator = models::UserAuthenticator {
            id: 1,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            user_id: 2,
            totp_secret: "encrypted-test-seed".to_string(),
        };
        let output = serde_json::to_value(authenticator).unwrap();
        assert!(output.get("totp_secret").is_none());
        assert!(output.get("otp_url").is_none());
    }
}

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
        let (id, created_at, updated_at, user_id, totp_secret): (
            i64,
            chrono::DateTime<chrono::Utc>,
            chrono::DateTime<chrono::Utc>,
            Option<i64>,
            String,
        ) = sqlx::query_as(
            r#"
            SELECT id, created_at, updated_at, user_id, totp_secret
            FROM user_authenticators
            WHERE user_id = $1 AND deleted_at IS NULL
            "#,
        )
        .bind(self.user_id)
        .fetch_one(executor)
        .await?;

        Ok(models::UserAuthenticator {
            id,
            created_at,
            updated_at,
            user_id: user_id.unwrap_or(0),
            totp_secret,
        })
    }
}
