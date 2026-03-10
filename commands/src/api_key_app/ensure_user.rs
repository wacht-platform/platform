use common::error::AppError;
use queries::api_key::GetApiAuthAppBySlugQuery;

use super::create::CreateApiAuthAppCommand;

pub struct EnsureUserApiAuthAppCommand {
    pub deployment_id: i64,
    pub user_id: i64,
}

impl EnsureUserApiAuthAppCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }
}

impl EnsureUserApiAuthAppCommand {
    pub async fn execute_with_db<'a, Db>(self, db: Db) -> Result<String, AppError>
    where
        Db: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut tx = db.begin().await?;
        if self.user_id <= 0 {
            return Err(AppError::BadRequest(
                "user_id must be a positive integer".to_string(),
            ));
        }

        let expected_slug = format!("oauth_{}", self.user_id);

        let existing = GetApiAuthAppBySlugQuery::new(self.deployment_id, expected_slug.clone())
            .execute_with_db(&mut *tx)
            .await?;

        if let Some(app) = existing {
            return Ok(app.app_slug);
        }

        let create_result = CreateApiAuthAppCommand::new(
            self.deployment_id,
            Some(self.user_id),
            expected_slug.clone(),
            format!("OAuth identity for user {}", self.user_id),
            "sk_live".to_string(),
        )
        .execute_with_db(tx.as_mut())
        .await;

        let result = match create_result {
            Ok(created) => created.app_slug,
            Err(AppError::Database(sqlx::Error::Database(db_err)))
                if db_err.code().as_deref() == Some("23505") =>
            {
                expected_slug
            }
            Err(err) => return Err(err),
        };

        tx.commit().await?;
        Ok(result)
    }
}
