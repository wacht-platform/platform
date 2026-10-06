use chrono::{DateTime, Utc};

use common::error::AppError;
use dto::json::{AddEmailRequest, AddPhoneRequest, UpdateEmailRequest, UpdatePhoneRequest};
use models::{UserEmailAddress, UserPhoneNumber, VerificationStrategy};

#[cfg(test)]
mod tests {
    #[test]
    fn every_identifier_mutation_checks_deployment_ownership() {
        let source = include_str!("user_identifiers.rs");
        let implementations: Vec<_> = source
            .split("impl ")
            .skip(1)
            .filter(|part| {
                part.starts_with("AddUser")
                    || part.starts_with("UpdateUser")
                    || part.starts_with("DeleteUser")
            })
            .collect();
        assert_eq!(implementations.len(), 7);
        for implementation in implementations {
            assert!(implementation.contains("FROM users") || implementation.contains("JOIN users"));
            assert!(implementation.contains("deployment_id = $"));
            assert!(implementation.contains(".bind(self.deployment_id)"));
        }
    }
}

const EMAIL_NOT_FOUND: &str = "Email not found";
const PHONE_NOT_FOUND: &str = "Phone number not found";
const USER_NOT_FOUND: &str = "User not found";
const SOCIAL_CONNECTION_NOT_FOUND: &str = "Social connection not found";

fn require_id(value: Option<i64>, field: &'static str) -> Result<i64, AppError> {
    value.ok_or_else(|| AppError::Validation(format!("{field} is required")))
}

#[derive(sqlx::FromRow)]
struct EmailRow {
    id: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deployment_id: Option<i64>,
    user_id: Option<i64>,
    email: String,
    is_primary: bool,
    verified: bool,
    verified_at: Option<DateTime<Utc>>,
    verification_strategy: Option<String>,
}

impl EmailRow {
    fn into_model(self, deployment_id: i64, user_id: i64) -> UserEmailAddress {
        UserEmailAddress {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            deployment_id: self.deployment_id.unwrap_or(deployment_id),
            user_id: self.user_id.unwrap_or(user_id),
            email: self.email,
            is_primary: self.is_primary,
            verified: self.verified,
            verified_at: self.verified_at.unwrap_or_else(Utc::now),
            verification_strategy: self
                .verification_strategy
                .and_then(|s| s.parse().ok())
                .unwrap_or(VerificationStrategy::Otp),
        }
    }
}

#[derive(sqlx::FromRow)]
struct PhoneRow {
    id: i64,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    user_id: Option<i64>,
    phone_number: String,
    country_code: String,
    verified: bool,
    verified_at: Option<DateTime<Utc>>,
}

impl PhoneRow {
    fn into_model(self, user_id: i64) -> UserPhoneNumber {
        UserPhoneNumber {
            id: self.id,
            created_at: self.created_at,
            updated_at: self.updated_at,
            user_id: self.user_id.unwrap_or(user_id),
            phone_number: self.phone_number,
            country_code: self.country_code,
            verified: self.verified,
            verified_at: self.verified_at.unwrap_or_else(Utc::now),
        }
    }
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

        let row = sqlx::query_as::<_, EmailRow>(
            r#"
            WITH target_user AS (
                SELECT id
                FROM users
                WHERE id = $5 AND deployment_id = $4
            ),
            cleared_primary AS (
                UPDATE user_email_addresses
                SET is_primary = false
                WHERE user_id IN (SELECT id FROM target_user)
                  AND $7 = true
            ),
            inserted_email AS (
                INSERT INTO user_email_addresses (
                    id, created_at, updated_at, deployment_id, user_id,
                    email_address, is_primary, verified, verified_at, verification_strategy
                )
                SELECT $1, $2, $3, $4, id, $6, $7, $8, $9, $10
                FROM target_user
                RETURNING
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    user_id,
                    email_address AS email,
                    is_primary,
                    verified,
                    verified_at,
                    verification_strategy
            ),
            updated_user AS (
                UPDATE users
                SET primary_email_address_id = (SELECT id FROM inserted_email)
                WHERE id IN (SELECT id FROM target_user)
                  AND $7 = true
            )
            SELECT *
            FROM inserted_email
            "#,
        )
        .bind(email_id)
        .bind(now)
        .bind(now)
        .bind(self.deployment_id)
        .bind(self.user_id)
        .bind(&self.request.email)
        .bind(is_primary)
        .bind(verified)
        .bind(now)
        .bind("otp")
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound(USER_NOT_FOUND.to_string()))?;

        Ok(row.into_model(self.deployment_id, self.user_id))
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
        let row = sqlx::query_as::<_, EmailRow>(
            r#"
            WITH target_email AS (
                SELECT e.id, e.user_id
                FROM user_email_addresses e
                JOIN users u ON u.id = e.user_id
                WHERE e.id = $1
                  AND e.user_id = $2
                  AND u.deployment_id = $6
            ),
            updated_user AS (
                UPDATE users
                SET primary_email_address_id = $1
                WHERE id IN (SELECT user_id FROM target_email)
                  AND $5 = true
            ),
            updated_email AS (
                UPDATE user_email_addresses
                SET
                    updated_at = NOW(),
                    email_address = COALESCE($3, email_address),
                    verified = COALESCE($4, verified),
                    verified_at = CASE WHEN COALESCE($4, false) = true THEN NOW() ELSE verified_at END
                WHERE id IN (SELECT id FROM target_email)
                RETURNING
                    id,
                    created_at,
                    updated_at,
                    deployment_id,
                    user_id,
                    email_address AS email,
                    is_primary,
                    verified,
                    verified_at,
                    verification_strategy
            )
            SELECT *
            FROM updated_email
            "#,
        )
        .bind(self.email_id)
        .bind(self.user_id)
        .bind(&self.request.email)
        .bind(self.request.verified)
        .bind(is_primary)
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound(EMAIL_NOT_FOUND.to_string()))?;

        Ok(row.into_model(self.deployment_id, self.user_id))
    }
}

pub struct DeleteUserEmailCommand {
    deployment_id: i64,
    user_id: i64,
    email_id: i64,
}

impl DeleteUserEmailCommand {
    pub fn new(deployment_id: i64, user_id: i64, email_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            email_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query(
            r#"
            WITH target_email AS (
                SELECT e.id
                FROM user_email_addresses e
                JOIN users u ON u.id = e.user_id
                WHERE e.id = $2
                  AND e.user_id = $1
                  AND u.deployment_id = $3
            ),
            deleted_social AS (
                DELETE FROM social_connections
                WHERE user_id = $1
                  AND user_email_address_id IN (SELECT id FROM target_email)
            )
            DELETE FROM user_email_addresses
            WHERE id IN (SELECT id FROM target_email)
            "#,
        )
        .bind(self.user_id)
        .bind(self.email_id)
        .bind(self.deployment_id)
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(EMAIL_NOT_FOUND.to_string()));
        }

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

        let row = sqlx::query_as::<_, PhoneRow>(
            r#"
            WITH target_user AS (
                SELECT id
                FROM users
                WHERE id = $4 AND deployment_id = $10
            ),
            inserted_phone AS (
                INSERT INTO user_phone_numbers (
                    id, created_at, updated_at, user_id, can_use_for_second_factor,
                    phone_number, country_code, verified, verified_at, deployment_id
                )
                SELECT $1, $2, $3, id, $5, $6, $7, $8, $9, $10
                FROM target_user
                RETURNING id, created_at, updated_at, user_id, phone_number, country_code, verified, verified_at
            ),
            updated_user AS (
                UPDATE users
                SET primary_phone_number_id = (SELECT id FROM inserted_phone)
                WHERE id IN (SELECT id FROM target_user)
                  AND $11 = true
            )
            SELECT * FROM inserted_phone
            "#,
        )
        .bind(phone_id)
        .bind(now)
        .bind(now)
        .bind(self.user_id)
        .bind(false)
        .bind(&self.request.phone_number)
        .bind(&self.request.country_code)
        .bind(verified)
        .bind(if verified { Some(now) } else { None })
        .bind(self.deployment_id)
        .bind(is_primary)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound(USER_NOT_FOUND.to_string()))?;

        Ok(row.into_model(self.user_id))
    }
}

pub struct UpdateUserPhoneCommand {
    deployment_id: i64,
    user_id: i64,
    phone_id: i64,
    request: UpdatePhoneRequest,
}

impl UpdateUserPhoneCommand {
    pub fn new(
        deployment_id: i64,
        user_id: i64,
        phone_id: i64,
        request: UpdatePhoneRequest,
    ) -> Self {
        Self {
            deployment_id,
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
        let row = sqlx::query_as::<_, PhoneRow>(
            r#"
            WITH target_phone AS (
                SELECT p.id, p.user_id
                FROM user_phone_numbers p
                JOIN users u ON u.id = p.user_id
                WHERE p.id = $1
                  AND p.user_id = $2
                  AND u.deployment_id = $7
            ),
            updated_user AS (
                UPDATE users
                SET primary_phone_number_id = $1
                WHERE id IN (SELECT user_id FROM target_phone)
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
                WHERE id IN (SELECT id FROM target_phone)
                RETURNING id, created_at, updated_at, user_id, phone_number, country_code, verified, verified_at
            )
            SELECT id, created_at, updated_at, user_id, phone_number, country_code, verified, verified_at
            FROM updated_phone
            "#,
        )
        .bind(self.phone_id)
        .bind(self.user_id)
        .bind(&self.request.phone_number)
        .bind(&self.request.country_code)
        .bind(self.request.verified)
        .bind(is_primary)
        .bind(self.deployment_id)
        .fetch_optional(executor)
        .await?
        .ok_or_else(|| AppError::NotFound(PHONE_NOT_FOUND.to_string()))?;

        Ok(row.into_model(self.user_id))
    }
}

pub struct DeleteUserPhoneCommand {
    deployment_id: i64,
    user_id: i64,
    phone_id: i64,
}

impl DeleteUserPhoneCommand {
    pub fn new(deployment_id: i64, user_id: i64, phone_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            phone_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query(
            r#"
            DELETE FROM user_phone_numbers
            WHERE id = $1
              AND user_id = $2
              AND EXISTS (SELECT 1 FROM users WHERE id = $2 AND deployment_id = $3)
            "#,
        )
        .bind(self.phone_id)
        .bind(self.user_id)
        .bind(self.deployment_id)
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(PHONE_NOT_FOUND.to_string()));
        }

        Ok(())
    }
}

pub struct DeleteUserSocialConnectionCommand {
    deployment_id: i64,
    user_id: i64,
    connection_id: i64,
}

impl DeleteUserSocialConnectionCommand {
    pub fn new(deployment_id: i64, user_id: i64, connection_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
            connection_id,
        }
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query(
            r#"
            DELETE FROM social_connections
            WHERE id = $1
              AND user_id = $2
              AND EXISTS (SELECT 1 FROM users WHERE id = $2 AND deployment_id = $3)
            "#,
        )
        .bind(self.connection_id)
        .bind(self.user_id)
        .bind(self.deployment_id)
        .execute(executor)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound(SOCIAL_CONNECTION_NOT_FOUND.to_string()));
        }

        Ok(())
    }
}
