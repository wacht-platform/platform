use chrono::Utc;

use common::error::AppError;
use dto::json::{AddEmailRequest, AddPhoneRequest, UpdateEmailRequest, UpdatePhoneRequest};
use models::{UserEmailAddress, UserPhoneNumber, VerificationStrategy};

const EMAIL_NOT_FOUND: &str = "Email not found";
const PHONE_NOT_FOUND: &str = "Phone number not found";

fn require_id(value: Option<i64>, field: &'static str) -> Result<i64, AppError> {
    value.ok_or_else(|| AppError::Validation(format!("{field} is required")))
}

pub struct AddUserEmailCommand {
    email_id: Option<i64>,
    deployment_id: i64,
    user_id: i64,
    request: AddEmailRequest,
}

impl AddUserEmailCommand {
    pub fn new(deployment_id: i64, user_id: i64, request: AddEmailRequest) -> Self {
        Self {
            email_id: None,
            deployment_id,
            user_id,
            request,
        }
    }

    pub fn with_email_id(mut self, email_id: i64) -> Self {
        self.email_id = Some(email_id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<UserEmailAddress, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let email_id = require_id(self.email_id, "email_id")?;
        let now = Utc::now();
        let verified = self.request.verified.unwrap_or(false);
        let is_primary = self.request.is_primary.unwrap_or(false);

        let row = sqlx::query!(
            r#"
            WITH cleared_primary AS (
                UPDATE user_email_addresses
                SET is_primary = false
                WHERE user_id = $5
                  AND $7 = true
            ),
            inserted_email AS (
                INSERT INTO user_email_addresses (
                    id, created_at, updated_at, deployment_id, user_id,
                    email_address, is_primary, verified, verified_at, verification_strategy
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                RETURNING
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    user_id,
                    email_address as "email!",
                    is_primary,
                    verified,
                    verified_at as "verified_at!",
                    verification_strategy as "verification_strategy: VerificationStrategy"
            ),
            updated_user AS (
                UPDATE users
                SET primary_email_address_id = (SELECT id FROM inserted_email)
                WHERE id = $5
                  AND $7 = true
            )
            SELECT *
            FROM inserted_email
            "#,
            email_id,
            now,
            now,
            self.deployment_id,
            self.user_id,
            self.request.email,
            is_primary,
            verified,
            now,
            "otp"
        )
        .fetch_one(executor)
        .await?;

        Ok(UserEmailAddress {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id.unwrap_or(self.deployment_id),
            user_id: row.user_id.unwrap_or(self.user_id),
            email: row.email,
            is_primary: row.is_primary,
            verified: row.verified,
            verified_at: row.verified_at,
            verification_strategy: row
                .verification_strategy
                .unwrap_or(VerificationStrategy::Otp),
        })
    }
}

pub struct UpdateUserEmailCommand {
    deployment_id: i64,
    user_id: i64,
    email_id: i64,
    request: UpdateEmailRequest,
}

impl UpdateUserEmailCommand {
    pub fn new(
        deployment_id: i64,
        user_id: i64,
        email_id: i64,
        request: UpdateEmailRequest,
    ) -> Self {
        Self {
            deployment_id,
            user_id,
            email_id,
            request,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<UserEmailAddress, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let is_primary = self.request.is_primary.unwrap_or(false);
        let row = sqlx::query!(
            r#"
            WITH updated_user AS (
                UPDATE users
                SET primary_email_address_id = $1
                WHERE id = $2
                  AND $5 = true
            ),
            updated_email AS (
                UPDATE user_email_addresses
                SET
                    updated_at = NOW(),
                    email_address = COALESCE($3, email_address),
                    verified = COALESCE($4, verified),
                    verified_at = CASE WHEN COALESCE($4, false) = true THEN NOW() ELSE verified_at END
                WHERE id = $1
                  AND user_id = $2
                RETURNING
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    user_id,
                    email_address as email,
                    is_primary,
                    verified,
                    verified_at,
                    verification_strategy
            )
            SELECT
                id,
                created_at,
                updated_at,
                deployment_id,
                user_id,
                email as "email!",
                is_primary,
                verified,
                verified_at,
                verification_strategy as "verification_strategy: VerificationStrategy"
            FROM updated_email
            "#,
            self.email_id,
            self.user_id,
            self.request.email,
            self.request.verified,
            is_primary
        )
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound(EMAIL_NOT_FOUND.to_string()))?;

        Ok(UserEmailAddress {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id.unwrap_or(self.deployment_id),
            user_id: row.user_id.unwrap_or(self.user_id),
            email: row.email,
            is_primary: row.is_primary,
            verified: row.verified,
            verified_at: row.verified_at.unwrap_or_else(Utc::now),
            verification_strategy: row
                .verification_strategy
                .unwrap_or(VerificationStrategy::Otp),
        })
    }
}

pub struct DeleteUserEmailCommand {
    user_id: i64,
    email_id: i64,
}

impl DeleteUserEmailCommand {
    pub fn new(user_id: i64, email_id: i64) -> Self {
        Self { user_id, email_id }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            r#"
            WITH deleted_social AS (
                DELETE FROM social_connections
                WHERE user_id = $1
                  AND user_email_address_id = $2
            )
            DELETE FROM user_email_addresses
            WHERE id = $2
              AND user_id = $1
            "#,
            self.user_id,
            self.email_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct AddUserPhoneCommand {
    phone_id: Option<i64>,
    deployment_id: i64,
    user_id: i64,
    request: AddPhoneRequest,
}

impl AddUserPhoneCommand {
    pub fn new(deployment_id: i64, user_id: i64, request: AddPhoneRequest) -> Self {
        Self {
            phone_id: None,
            deployment_id,
            user_id,
            request,
        }
    }

    pub fn with_phone_id(mut self, phone_id: i64) -> Self {
        self.phone_id = Some(phone_id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<UserPhoneNumber, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let phone_id = require_id(self.phone_id, "phone_id")?;
        let now = Utc::now();
        let verified = self.request.verified.unwrap_or(false);
        let is_primary = self.request.is_primary.unwrap_or(false);

        let row = sqlx::query!(
            r#"
            WITH inserted_phone AS (
                INSERT INTO user_phone_numbers (
                    id, created_at, updated_at, user_id, can_use_for_second_factor,
                    phone_number, country_code, verified, verified_at, deployment_id
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
                RETURNING id, created_at, updated_at, user_id, phone_number, country_code, verified, verified_at
            ),
            updated_user AS (
                UPDATE users
                SET primary_phone_number_id = (SELECT id FROM inserted_phone)
                WHERE id = $4
                  AND $11 = true
            )
            SELECT * FROM inserted_phone
            "#,
            phone_id,
            now,
            now,
            self.user_id,
            false,
            self.request.phone_number,
            self.request.country_code,
            verified,
            if verified { Some(now) } else { None },
            self.deployment_id,
            is_primary
        )
        .fetch_one(executor)
        .await?;

        Ok(UserPhoneNumber {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            user_id: row.user_id.unwrap_or(self.user_id),
            phone_number: row.phone_number,
            country_code: row.country_code,
            verified: row.verified,
            verified_at: row.verified_at.unwrap_or_else(Utc::now),
        })
    }
}

pub struct UpdateUserPhoneCommand {
    user_id: i64,
    phone_id: i64,
    request: UpdatePhoneRequest,
}

impl UpdateUserPhoneCommand {
    pub fn new(user_id: i64, phone_id: i64, request: UpdatePhoneRequest) -> Self {
        Self {
            user_id,
            phone_id,
            request,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<UserPhoneNumber, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let is_primary = self.request.is_primary.unwrap_or(false);
        let row = sqlx::query!(
            r#"
            WITH updated_user AS (
                UPDATE users
                SET primary_phone_number_id = $1
                WHERE id = $2
                  AND $6 = true
            ),
            updated_phone AS (
                UPDATE user_phone_numbers
                SET
                    updated_at = NOW(),
                    phone_number = COALESCE($3, phone_number),
                    country_code = COALESCE($4, country_code),
                    verified = COALESCE($5, verified),
                    verified_at = CASE WHEN COALESCE($5, false) = true THEN NOW() ELSE verified_at END
                WHERE id = $1
                  AND user_id = $2
                RETURNING id, created_at, updated_at, user_id, phone_number, country_code, verified, verified_at
            )
            SELECT id, created_at, updated_at, user_id, phone_number, country_code, verified, verified_at
            FROM updated_phone
            "#,
            self.phone_id,
            self.user_id,
            self.request.phone_number,
            self.request.country_code,
            self.request.verified,
            is_primary
        )
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound(PHONE_NOT_FOUND.to_string()))?;

        Ok(UserPhoneNumber {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            user_id: row.user_id.unwrap_or(self.user_id),
            phone_number: row.phone_number,
            country_code: row.country_code,
            verified: row.verified,
            verified_at: row.verified_at.unwrap_or_else(Utc::now),
        })
    }
}

pub struct DeleteUserPhoneCommand {
    user_id: i64,
    phone_id: i64,
}

impl DeleteUserPhoneCommand {
    pub fn new(user_id: i64, phone_id: i64) -> Self {
        Self { user_id, phone_id }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            "DELETE FROM user_phone_numbers WHERE id = $1 AND user_id = $2",
            self.phone_id,
            self.user_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

pub struct DeleteUserSocialConnectionCommand {
    user_id: i64,
    connection_id: i64,
}

impl DeleteUserSocialConnectionCommand {
    pub fn new(user_id: i64, connection_id: i64) -> Self {
        Self {
            user_id,
            connection_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            "DELETE FROM social_connections WHERE id = $1 AND user_id = $2",
            self.connection_id,
            self.user_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}
