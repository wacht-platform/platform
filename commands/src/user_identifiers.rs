use chrono::Utc;

use crate::Command;
use common::error::AppError;
use common::state::AppState;
use dto::json::{AddEmailRequest, AddPhoneRequest, UpdateEmailRequest, UpdatePhoneRequest};
use models::{UserEmailAddress, UserPhoneNumber, VerificationStrategy};

pub struct AddUserEmailCommand {
    deployment_id: i64,
    user_id: i64,
    request: AddEmailRequest,
}

impl AddUserEmailCommand {
    pub fn new(deployment_id: i64, user_id: i64, request: AddEmailRequest) -> Self {
        Self {
            deployment_id,
            user_id,
            request,
        }
    }
}

impl Command for AddUserEmailCommand {
    type Output = UserEmailAddress;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let email_id = app_state.sf.next_id()? as i64;
        let verified = self.request.verified.unwrap_or(false);
        let is_primary = self.request.is_primary.unwrap_or(false);

        if is_primary {
            sqlx::query!(
                "UPDATE user_email_addresses SET is_primary = false WHERE user_id = $1",
                self.user_id
            )
            .execute(&app_state.db_pool)
            .await?;
        }

        sqlx::query!(
            r#"
            INSERT INTO user_email_addresses (
                id, created_at, updated_at, deployment_id, user_id,
                email_address, is_primary, verified, verified_at, verification_strategy
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            email_id,
            now,
            now,
            self.deployment_id,
            self.user_id,
            self.request.email,
            is_primary,
            verified,
            if verified { now } else { now },
            "otp"
        )
        .execute(&app_state.db_pool)
        .await?;

        if is_primary {
            sqlx::query!(
                "UPDATE users SET primary_email_address_id = $1 WHERE id = $2",
                email_id,
                self.user_id
            )
            .execute(&app_state.db_pool)
            .await?;
        }

        Ok(UserEmailAddress {
            id: email_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            user_id: self.user_id,
            email: self.request.email,
            is_primary,
            verified,
            verified_at: now,
            verification_strategy: VerificationStrategy::Otp,
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
}

impl Command for UpdateUserEmailCommand {
    type Output = UserEmailAddress;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        match (&self.request.email, self.request.verified) {
            (Some(email), Some(verified)) => {
                sqlx::query!(
                    r#"
                    UPDATE user_email_addresses
                    SET updated_at = NOW(), email_address = $1, verified = $2,
                        verified_at = CASE WHEN $2 = true THEN NOW() ELSE verified_at END
                    WHERE id = $3 AND user_id = $4
                    "#,
                    email,
                    verified,
                    self.email_id,
                    self.user_id
                )
                .execute(&app_state.db_pool)
                .await?;
            }
            (Some(email), None) => {
                sqlx::query!(
                    r#"
                    UPDATE user_email_addresses
                    SET updated_at = NOW(), email_address = $1
                    WHERE id = $2 AND user_id = $3
                    "#,
                    email,
                    self.email_id,
                    self.user_id
                )
                .execute(&app_state.db_pool)
                .await?;
            }
            (None, Some(verified)) => {
                sqlx::query!(
                    r#"
                    UPDATE user_email_addresses
                    SET updated_at = NOW(), verified = $1,
                        verified_at = CASE WHEN $1 = true THEN NOW() ELSE verified_at END
                    WHERE id = $2 AND user_id = $3
                    "#,
                    verified,
                    self.email_id,
                    self.user_id
                )
                .execute(&app_state.db_pool)
                .await?;
            }
            (None, None) => (),
        }

        if let Some(true) = self.request.is_primary {
            sqlx::query!(
                r#"
                UPDATE users
                SET primary_email_address_id = $1
                WHERE id = $2
                "#,
                self.email_id,
                self.user_id
            )
            .execute(&app_state.db_pool)
            .await?;
        }

        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id, user_id,
                   email_address as email, is_primary, verified, verified_at, verification_strategy as "verification_strategy: VerificationStrategy"
            FROM user_email_addresses
            WHERE id = $1 AND user_id = $2
            "#,
            self.email_id,
            self.user_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(UserEmailAddress {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            deployment_id: row.deployment_id.unwrap_or(self.deployment_id),
            user_id: row.user_id.unwrap_or(self.user_id),
            email: row.email.unwrap_or_default(),
            is_primary: row.is_primary,
            verified: row.verified,
            verified_at: row.verified_at.unwrap_or_else(|| chrono::Utc::now()),
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
}

impl Command for DeleteUserEmailCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await?;

        sqlx::query!(
            "DELETE FROM social_connections WHERE user_id = $1 AND user_email_address_id = $2",
            self.user_id,
            self.email_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM user_email_addresses WHERE id = $1 AND user_id = $2",
            self.email_id,
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}

pub struct AddUserPhoneCommand {
    deployment_id: i64,
    user_id: i64,
    request: AddPhoneRequest,
}

impl AddUserPhoneCommand {
    pub fn new(deployment_id: i64, user_id: i64, request: AddPhoneRequest) -> Self {
        Self {
            deployment_id,
            user_id,
            request,
        }
    }
}

impl Command for AddUserPhoneCommand {
    type Output = UserPhoneNumber;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let phone_id = app_state.sf.next_id()? as i64;
        let verified = self.request.verified.unwrap_or(false);
        let is_primary = self.request.is_primary.unwrap_or(false);

        sqlx::query!(
            r#"
            INSERT INTO user_phone_numbers (
                id, created_at, updated_at, user_id, can_use_for_second_factor,
                phone_number, country_code, verified, verified_at, deployment_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
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
        )
        .execute(&app_state.db_pool)
        .await?;

        if is_primary {
            sqlx::query!(
                "UPDATE users SET primary_phone_number_id = $1 WHERE id = $2",
                phone_id,
                self.user_id
            )
            .execute(&app_state.db_pool)
            .await?;
        }

        Ok(UserPhoneNumber {
            id: phone_id,
            created_at: now,
            updated_at: now,
            user_id: self.user_id,
            phone_number: self.request.phone_number,
            country_code: self.request.country_code,
            verified,
            verified_at: now,
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
}

impl Command for UpdateUserPhoneCommand {
    type Output = UserPhoneNumber;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Handle is_primary first
        if let Some(is_primary) = self.request.is_primary {
            if is_primary {
                sqlx::query!(
                    "UPDATE users SET primary_phone_number_id = $1 WHERE id = $2",
                    self.phone_id,
                    self.user_id
                )
                .execute(&app_state.db_pool)
                .await?;
            }
        }

        // Update phone number and/or country code
        match (&self.request.phone_number, &self.request.country_code) {
            (Some(phone_number), Some(country_code)) => {
                sqlx::query!(
                    r#"
                    UPDATE user_phone_numbers
                    SET updated_at = NOW(), phone_number = $1, country_code = $2
                    WHERE id = $3 AND user_id = $4
                    "#,
                    phone_number,
                    country_code,
                    self.phone_id,
                    self.user_id
                )
                .execute(&app_state.db_pool)
                .await?;
            }
            (Some(phone_number), None) => {
                sqlx::query!(
                    r#"
                    UPDATE user_phone_numbers
                    SET updated_at = NOW(), phone_number = $1
                    WHERE id = $2 AND user_id = $3
                    "#,
                    phone_number,
                    self.phone_id,
                    self.user_id
                )
                .execute(&app_state.db_pool)
                .await?;
            }
            (None, Some(country_code)) => {
                sqlx::query!(
                    r#"
                    UPDATE user_phone_numbers
                    SET updated_at = NOW(), country_code = $1
                    WHERE id = $2 AND user_id = $3
                    "#,
                    country_code,
                    self.phone_id,
                    self.user_id
                )
                .execute(&app_state.db_pool)
                .await?;
            }
            (None, None) => {
                // No phone number or country code update
            }
        }

        // Update verified status
        if let Some(verified) = self.request.verified {
            sqlx::query!(
                r#"
                UPDATE user_phone_numbers
                SET updated_at = NOW(), verified = $1,
                    verified_at = CASE WHEN $1 = true THEN NOW() ELSE verified_at END
                WHERE id = $2 AND user_id = $3
                "#,
                verified,
                self.phone_id,
                self.user_id
            )
            .execute(&app_state.db_pool)
            .await?;
        }

        // Fetch and return the updated record
        let row = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, user_id,
                   phone_number, country_code, verified, verified_at
            FROM user_phone_numbers
            WHERE id = $1 AND user_id = $2
            "#,
            self.phone_id,
            self.user_id
        )
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(UserPhoneNumber {
            id: row.id,
            created_at: row.created_at,
            updated_at: row.updated_at,
            user_id: row.user_id.unwrap_or(self.user_id),
            phone_number: row.phone_number,
            country_code: row.country_code,
            verified: row.verified,
            verified_at: row.verified_at.unwrap_or_else(|| chrono::Utc::now()),
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
}

impl Command for DeleteUserPhoneCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "DELETE FROM user_phone_numbers WHERE id = $1 AND user_id = $2",
            self.phone_id,
            self.user_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

// Social Connection Commands
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
}

impl Command for DeleteUserSocialConnectionCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "DELETE FROM social_connections WHERE id = $1 AND user_id = $2",
            self.connection_id,
            self.user_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}
