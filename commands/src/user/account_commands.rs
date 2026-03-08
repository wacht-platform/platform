use chrono::Utc;
use serde_json::json;
use sqlx::Execute;

use common::utils::{security::PasswordHasher, validation::UserValidator};
use common::{HasDbRouter, HasIdProvider, error::AppError};
use dto::json::{CreateUserRequest, UpdateUserRequest};
use models::{UserDetails, UserWithIdentifiers};
use queries::{GetDeploymentAuthSettingsQuery, GetUserDetailsQuery};

pub struct CreateUserCommand {
    deployment_id: i64,
    request: CreateUserRequest,
}

impl CreateUserCommand {
    pub fn new(deployment_id: i64, request: CreateUserRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<UserWithIdentifiers, AppError>
    where
        D: HasDbRouter + HasIdProvider,
    {
        let now = Utc::now();
        let ids = (
            deps.id_provider().next_id()? as i64,
            self.request
                .email_address
                .as_ref()
                .map(|_| deps.id_provider().next_id().map(|id| id as i64))
                .transpose()?,
            self.request
                .phone_number
                .as_ref()
                .map(|_| deps.id_provider().next_id().map(|id| id as i64))
                .transpose()?,
        );
        let user_id = ids.0;

        let auth_settings = GetDeploymentAuthSettingsQuery::new(self.deployment_id)
            .execute_with_db(
                deps.db_router()
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?;

        let mut tx = deps.db_router().writer().begin().await?;

        UserValidator::validate_user_creation(
            &self.request.first_name,
            &self.request.last_name,
            &self.request.email_address,
            &self.request.phone_number,
            &self.request.username,
            &self.request.password,
            self.request.skip_password_check,
            &auth_settings,
        )
        .map_err(|errors| {
            let error_messages: Vec<String> = errors
                .into_iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect();
            AppError::BadRequest(format!("Validation failed: {}", error_messages.join(", ")))
        })?;

        let hashed_password = if let Some(password) = &self.request.password {
            Some(PasswordHasher::hash_password(password)?)
        } else {
            None
        };

        sqlx::query!(
            r#"
            INSERT INTO users (
                id, created_at, updated_at, first_name, last_name, username,
                password, profile_picture_url, has_profile_picture, schema_version, disabled, second_factor_policy,
                deployment_id, public_metadata, private_metadata, backup_codes, backup_codes_generated
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            "#,
            user_id,
            now,
            now,
            self.request.first_name,
            self.request.last_name,
            self.request.username.as_deref().unwrap_or(""),
            hashed_password.as_deref(),
            "",
            false, // has_profile_picture defaults to false
            "v1",
            false,
            "optional",
            self.deployment_id,
            json!({}),
            json!({}),
            &Vec::<String>::new(),
            false // backup_codes_generated defaults to false
        )
        .execute(&mut *tx)
        .await?;

        let mut primary_email_address = None;
        let mut primary_phone_number = None;

        if let Some(email) = &self.request.email_address {
            let email_id = ids.1.ok_or_else(|| {
                AppError::Internal("Missing email ID for user email insert".to_string())
            })?;

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
                user_id,
                email,
                true,
                true,
                now,
                "otp"
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "UPDATE users SET primary_email_address_id = $1 WHERE id = $2",
                email_id,
                user_id
            )
            .execute(&mut *tx)
            .await?;

            primary_email_address = Some(email.clone());
        }

        if let Some(phone) = &self.request.phone_number {
            let phone_id = ids.2.ok_or_else(|| {
                AppError::Internal("Missing phone ID for user phone insert".to_string())
            })?;

            sqlx::query!(
                r#"
            INSERT INTO user_phone_numbers (
                id, created_at, updated_at, user_id, can_use_for_second_factor,
                phone_number, verified, verified_at, deployment_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
                phone_id,
                now,
                now,
                user_id,
                false,
                phone,
                true,
                now,
                self.deployment_id,
            )
            .execute(&mut *tx)
            .await?;

            sqlx::query!(
                "UPDATE users SET primary_phone_number_id = $1 WHERE id = $2",
                phone_id,
                user_id
            )
            .execute(&mut *tx)
            .await?;

            primary_phone_number = Some(phone.clone());
        }

        let user = UserWithIdentifiers {
            id: user_id,
            created_at: now,
            updated_at: now,
            first_name: self.request.first_name,
            last_name: self.request.last_name,
            username: self.request.username,
            profile_picture_url: String::new(),
            primary_email_address,
            primary_phone_number,
        };

        tx.commit().await?;

        Ok(user)
    }
}

pub struct UpdateUserCommand {
    deployment_id: i64,
    user_id: i64,
    request: UpdateUserRequest,
}

impl UpdateUserCommand {
    pub fn new(deployment_id: i64, user_id: i64, request: UpdateUserRequest) -> Self {
        Self {
            deployment_id,
            user_id,
            request,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<UserDetails, AppError>
    where
        D: HasDbRouter,
    {
        let mut tx = deps.db_router().writer().begin().await?;

        // Build a single dynamic UPDATE query
        let mut query_builder = sqlx::QueryBuilder::new("UPDATE users SET updated_at = NOW()");

        if let Some(first_name) = &self.request.first_name {
            query_builder.push(", first_name = ");
            query_builder.push_bind(first_name);
        }

        if let Some(last_name) = &self.request.last_name {
            query_builder.push(", last_name = ");
            query_builder.push_bind(last_name);
        }

        if let Some(username) = &self.request.username {
            query_builder.push(", username = ");
            query_builder.push_bind(username);
        }

        if let Some(public_metadata) = &self.request.public_metadata {
            query_builder.push(", public_metadata = ");
            query_builder.push_bind(public_metadata);
        }

        if let Some(private_metadata) = &self.request.private_metadata {
            query_builder.push(", private_metadata = ");
            query_builder.push_bind(private_metadata);
        }

        if let Some(disabled) = self.request.disabled {
            query_builder.push(", disabled = ");
            query_builder.push_bind(disabled);
        }

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);
        query_builder.push(" AND id = ");
        query_builder.push_bind(self.user_id);
        query_builder.push(" RETURNING id");

        let mut query = query_builder.build();

        let arguments = query
            .take_arguments()
            .map_err(|e| AppError::Internal(format!("Failed to build query arguments: {}", e)))?;
        let sql = query.sql();

        if let Some(args) = arguments {
            let (_user_id,): (i64,) = sqlx::query_as_with(sql, args).fetch_one(&mut *tx).await?;
        } else {
            return Err(AppError::Internal(
                "Failed to construct query arguments".to_string(),
            ));
        }

        if let Some(true) = self.request.disabled {
            sqlx::query!("DELETE FROM signins WHERE user_id = $1", self.user_id)
                .execute(&mut *tx)
                .await?;
        }

        tx.commit().await?;

        let details_query = GetUserDetailsQuery::new(self.deployment_id, self.user_id);
        let user_details = details_query
            .execute_with_db(
                deps.db_router()
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await?;

        Ok(user_details)
    }
}

#[derive(Debug)]
pub struct UpdateUserProfileImageCommand {
    pub deployment_id: i64,
    pub user_id: i64,
    pub profile_picture_url: String,
}

impl UpdateUserProfileImageCommand {
    pub fn new(deployment_id: i64, user_id: i64, profile_picture_url: String) -> Self {
        Self {
            deployment_id,
            user_id,
            profile_picture_url,
        }
    }
}

impl UpdateUserProfileImageCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        sqlx::query!(
            "UPDATE users SET updated_at = NOW(), profile_picture_url = $1, has_profile_picture = true WHERE deployment_id = $2 AND id = $3",
            self.profile_picture_url,
            self.deployment_id,
            self.user_id
        )
        .execute(executor)
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct UpdateUserPasswordCommand {
    pub deployment_id: i64,
    pub user_id: i64,
    pub new_password: String,
    pub skip_password_check: bool,
}

impl UpdateUserPasswordCommand {
    pub fn new(
        deployment_id: i64,
        user_id: i64,
        new_password: String,
        skip_password_check: bool,
    ) -> Self {
        Self {
            deployment_id,
            user_id,
            new_password,
            skip_password_check,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter,
    {
        if !self.skip_password_check {
            let auth_settings = GetDeploymentAuthSettingsQuery::new(self.deployment_id)
                .execute_with_db(
                    deps.db_router()
                        .reader(common::db_router::ReadConsistency::Strong),
                )
                .await?;

            UserValidator::validate_password(
                &Some(self.new_password.clone()),
                &auth_settings.password,
            )
            .map_err(|errors| {
                let error_messages: Vec<String> = errors
                    .into_iter()
                    .map(|e| format!("{}: {}", e.field, e.message))
                    .collect();
                AppError::BadRequest(format!("Validation failed: {}", error_messages.join(", ")))
            })?;
        }

        let hashed_password = PasswordHasher::hash_password(&self.new_password)?;
        let writer = deps.db_router().writer();
        sqlx::query!(
            "UPDATE users SET updated_at = NOW(), password = $1 WHERE deployment_id = $2 AND id = $3",
            hashed_password,
            self.deployment_id,
            self.user_id
        )
        .execute(writer)
        .await?;

        Ok(())
    }
}

pub struct DeleteUserCommand {
    deployment_id: i64,
    user_id: i64,
}

impl DeleteUserCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }
}

impl DeleteUserCommand {
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            r#"
            WITH user_exists AS (
                SELECT id
                FROM users
                WHERE id = $1 AND deployment_id = $2
            ),
            deleted_social AS (DELETE FROM social_connections WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_emails AS (DELETE FROM user_email_addresses WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_phones AS (DELETE FROM user_phone_numbers WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_segments AS (DELETE FROM user_segments WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_authenticators AS (DELETE FROM user_authenticators WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_passkeys AS (DELETE FROM user_passkeys WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_notifications AS (DELETE FROM notifications WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_scim_ext AS (DELETE FROM scim_external_ids WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_scim_members AS (DELETE FROM scim_group_members WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_workspace_roles AS (
                DELETE FROM workspace_membership_roles
                WHERE workspace_membership_id IN (SELECT id FROM workspace_memberships WHERE user_id = $1)
                  AND EXISTS(SELECT 1 FROM user_exists)
            ),
            deleted_workspaces AS (DELETE FROM workspace_memberships WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_org_roles AS (
                DELETE FROM organization_membership_roles
                WHERE organization_membership_id IN (SELECT id FROM organization_memberships WHERE user_id = $1)
                  AND EXISTS(SELECT 1 FROM user_exists)
            ),
            deleted_orgs AS (DELETE FROM organization_memberships WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            updated_sessions AS (
                UPDATE sessions
                SET active_signin_id = NULL
                WHERE active_signin_id IN (SELECT id FROM signins WHERE user_id = $1)
                  AND EXISTS(SELECT 1 FROM user_exists)
            ),
            deleted_signins AS (DELETE FROM signins WHERE user_id = $1 AND EXISTS(SELECT 1 FROM user_exists)),
            deleted_user AS (DELETE FROM users WHERE id = $1 AND deployment_id = $2 AND EXISTS(SELECT 1 FROM user_exists))
            SELECT EXISTS(SELECT 1 FROM user_exists) AS "user_exists!"
            "#,
            self.user_id,
            self.deployment_id
        )
        .fetch_one(executor)
        .await?;

        if !result.user_exists {
            return Err(AppError::NotFound("User not found".to_string()));
        }

        Ok(())
    }
}
