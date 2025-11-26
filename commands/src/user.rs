use chrono::{Duration, Utc};
use serde_json::json;

use crate::{Command, SendEmailCommand};
use common::error::AppError;
use common::state::AppState;
use common::utils::{security::PasswordHasher, validation::UserValidator};
use dto::json::{CreateUserRequest, InviteUserRequest, UpdateUserRequest};
use models::{DeploymentInvitation, UserDetails, UserWithIdentifiers};
use queries::{GetDeploymentAuthSettingsQuery, Query};

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
}

impl Command for CreateUserCommand {
    type Output = UserWithIdentifiers;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let user_id = app_state.sf.next_id()? as i64;

        let auth_settings = GetDeploymentAuthSettingsQuery::new(self.deployment_id)
            .execute(app_state)
            .await?;

        let mut tx = app_state.db_pool.begin().await?;

        UserValidator::validate_user_creation(
            &self.request.first_name,
            &self.request.last_name,
            &self.request.email_address,
            &self.request.phone_number,
            &self.request.username,
            &self.request.password,
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
            let email_id = app_state.sf.next_id()? as i64;

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
            let phone_id = app_state.sf.next_id()? as i64;

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

pub struct InviteUserCommand {
    deployment_id: i64,
    request: InviteUserRequest,
}

impl InviteUserCommand {
    pub fn new(deployment_id: i64, request: InviteUserRequest) -> Self {
        Self {
            deployment_id,
            request,
        }
    }
}

impl Command for InviteUserCommand {
    type Output = DeploymentInvitation;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let expiry_days = self.request.expiry_days.unwrap_or(7);
        let expiry = now + Duration::days(expiry_days);
        let invitation_id = app_state.sf.next_id()? as i64;

        let token = {
            use rand::Rng;
            let mut rng = rand::rng();
            let token_bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
            format!("dep.{}", hex::encode(token_bytes))
        };

        sqlx::query!(
            r#"
            INSERT INTO deployment_invitations (
                id, created_at, updated_at, deployment_id,
                first_name, last_name, email_address, token, expiry
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            invitation_id,
            now,
            now,
            self.deployment_id,
            self.request.first_name,
            self.request.last_name,
            self.request.email_address,
            token,
            expiry
        )
        .execute(&app_state.db_pool)
        .await?;

        let variables = serde_json::json!({
            "app": {
                "name": "Your App",
                "logo": "https://via.placeholder.com/150"
            },
            "user": {
                "first_name": self.request.first_name.clone(),
                "last_name": self.request.last_name.clone()
            },
            "invitation": {
                "expires_in_days": expiry_days.to_string(),
                "token": token.clone()
            }
        });

        SendEmailCommand::new(
            self.deployment_id,
            "workspace_invite_template".to_string(),
            self.request.email_address.clone(),
            variables,
        )
        .execute(app_state)
        .await?;

        let invitation = DeploymentInvitation {
            id: invitation_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            first_name: self.request.first_name,
            last_name: self.request.last_name,
            email_address: self.request.email_address,
            token,
            expiry,
        };

        Ok(invitation)
    }
}

pub struct ApproveWaitlistUserCommand {
    deployment_id: i64,
    waitlist_user_id: i64,
}

impl ApproveWaitlistUserCommand {
    pub fn new(deployment_id: i64, waitlist_user_id: i64) -> Self {
        Self {
            deployment_id,
            waitlist_user_id,
        }
    }
}

impl Command for ApproveWaitlistUserCommand {
    type Output = DeploymentInvitation;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        let mut tx = app_state.db_pool.begin().await?;

        let waitlist_user = sqlx::query!(
            r#"
            SELECT id, created_at, updated_at, deployment_id,
                   first_name, last_name, email_address
            FROM deployment_waitlist_users
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.waitlist_user_id,
            self.deployment_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| AppError::NotFound("Waitlist user not found".to_string()))?;

        let invitation_id = app_state.sf.next_id()? as i64;
        let expiry = now + Duration::days(7);

        let first_name = waitlist_user.first_name.unwrap_or_default();
        let last_name = waitlist_user.last_name.unwrap_or_default();
        let email_address = waitlist_user.email_address.unwrap_or_default();

        // Generate secure token for waitlist approval - must be done before any await
        let token = {
            use rand::Rng;
            let mut rng = rand::rng();
            let token_bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
            format!("dep.{}", hex::encode(token_bytes))
        };

        sqlx::query!(
            r#"
            INSERT INTO deployment_invitations (
                id, created_at, updated_at, deployment_id,
                first_name, last_name, email_address, token, expiry
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            invitation_id,
            now,
            now,
            self.deployment_id,
            first_name,
            last_name,
            email_address,
            token.clone(),
            expiry
        )
        .execute(&mut *tx)
        .await?;

        let variables = serde_json::json!({
            "app": {
                "name": "Your App",
                "logo": "https://via.placeholder.com/150"
            },
            "user": {
                "first_name": first_name.clone(),
                "last_name": last_name.clone()
            },
            "invitation": {
                "expires_in_days": "7",
                "token": token.clone()
            }
        });

        SendEmailCommand::new(
            self.deployment_id,
            "waitlist_invite_template".to_string(),
            email_address.clone(),
            variables,
        )
        .execute(app_state)
        .await?;

        sqlx::query!(
            "DELETE FROM deployment_waitlist_users WHERE id = $1",
            self.waitlist_user_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        let invitation = DeploymentInvitation {
            id: invitation_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            first_name: first_name.clone(),
            last_name: last_name.clone(),
            email_address: email_address.clone(),
            token,
            expiry,
        };

        Ok(invitation)
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
}

impl Command for UpdateUserCommand {
    type Output = UserDetails;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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

        query_builder.push(" WHERE deployment_id = ");
        query_builder.push_bind(self.deployment_id);
        query_builder.push(" AND id = ");
        query_builder.push_bind(self.user_id);

        // Execute the single query
        query_builder.build().execute(&app_state.db_pool).await?;

        use queries::{GetUserDetailsQuery, Query};
        let user_details = GetUserDetailsQuery::new(self.deployment_id, self.user_id)
            .execute(app_state)
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

impl Command for UpdateUserProfileImageCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            "UPDATE users SET updated_at = NOW(), profile_picture_url = $1, has_profile_picture = true WHERE deployment_id = $2 AND id = $3",
            self.profile_picture_url,
            self.deployment_id,
            self.user_id
        )
        .execute(&app_state.db_pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct UpdateUserPasswordCommand {
    pub deployment_id: i64,
    pub user_id: i64,
    pub new_password: String,
}

impl UpdateUserPasswordCommand {
    pub fn new(deployment_id: i64, user_id: i64, new_password: String) -> Self {
        Self {
            deployment_id,
            user_id,
            new_password,
        }
    }
}

impl Command for UpdateUserPasswordCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Hash the new password
        let hashed_password = PasswordHasher::hash_password(&self.new_password)?;

        // Update the password
        sqlx::query!(
            "UPDATE users SET updated_at = NOW(), password = $1 WHERE deployment_id = $2 AND id = $3",
            hashed_password,
            self.deployment_id,
            self.user_id
        )
        .execute(&app_state.db_pool)
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

impl Command for DeleteUserCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut tx = app_state.db_pool.begin().await?;

        let exists = sqlx::query!(
            "SELECT id FROM users WHERE id = $1 AND deployment_id = $2",
            self.user_id,
            self.deployment_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        if exists.is_none() {
            return Err(AppError::NotFound("User not found".to_string()));
        }

        sqlx::query!(
            "DELETE FROM social_connections WHERE user_id = $1",
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM user_email_addresses WHERE user_id = $1",
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM user_phone_numbers WHERE user_id = $1",
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM organization_membership_roles WHERE organization_membership_id IN (SELECT id FROM organization_memberships WHERE user_id = $1)",
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM organization_memberships WHERE user_id = $1",
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM workspace_membership_roles WHERE workspace_membership_id IN (SELECT id FROM workspace_memberships WHERE user_id = $1)",
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM workspace_memberships WHERE user_id = $1",
            self.user_id
        )
        .execute(&mut *tx)
        .await?;

        sqlx::query!(
            "DELETE FROM users WHERE id = $1 AND deployment_id = $2",
            self.user_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(())
    }
}

use josekit::jws::{ES256, JwsHeader};
use josekit::jwt::{self, JwtPayload};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ImpersonationTokenClaims {
    pub user_id: i64,
    pub deployment_id: i64,
    #[serde(rename = "type")]
    pub token_type: String,
}

pub struct GenerateImpersonationTokenCommand {
    deployment_id: i64,
    user_id: i64,
}

impl GenerateImpersonationTokenCommand {
    pub fn new(deployment_id: i64, user_id: i64) -> Self {
        Self {
            deployment_id,
            user_id,
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct GenerateImpersonationTokenResponse {
    pub token: String,
    pub redirect_url: String,
}

impl Command for GenerateImpersonationTokenCommand {
    type Output = GenerateImpersonationTokenResponse;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Get deployment keypair
        let keypair = sqlx::query!(
            r#"
            SELECT private_key, public_key, frontend_host
            FROM deployment_key_pairs dk
            JOIN deployments d ON d.id = dk.deployment_id
            WHERE dk.deployment_id = $1
            "#,
            self.deployment_id
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to get deployment keypair: {}", e)))?;

        // Verify user exists and is not disabled
        let user = sqlx::query!(
            r#"
            SELECT id, disabled
            FROM users
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.user_id,
            self.deployment_id
        )
        .fetch_optional(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Internal(format!("Failed to fetch user: {}", e)))?
        .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

        if user.disabled {
            return Err(AppError::BadRequest(
                "Cannot impersonate disabled user".to_string(),
            ));
        }

        let private_key_pem = keypair.private_key;

        // Create JWT payload
        let mut payload = JwtPayload::new();
        payload.set_subject(&self.user_id.to_string());

        let now = std::time::SystemTime::now();
        let expires = now + std::time::Duration::from_secs(600);

        payload.set_issued_at(&now);
        payload.set_expires_at(&expires);

        // Add custom claims
        payload
            .set_claim("user_id", Some(serde_json::json!(self.user_id)))
            .map_err(|e| AppError::Internal(format!("Failed to set user_id claim: {}", e)))?;
        payload
            .set_claim("deployment_id", Some(serde_json::json!(self.deployment_id)))
            .map_err(|e| AppError::Internal(format!("Failed to set deployment_id claim: {}", e)))?;
        payload
            .set_claim("type", Some(serde_json::json!("impersonation")))
            .map_err(|e| AppError::Internal(format!("Failed to set type claim: {}", e)))?;

        // Sign the token
        let signer = ES256
            .signer_from_pem(&private_key_pem)
            .map_err(|e| AppError::Internal(format!("Failed to create signer: {}", e)))?;

        let mut header = JwsHeader::new();
        header.set_token_type("JWT");

        let token = jwt::encode_with_signer(&payload, &header, &signer)
            .map_err(|e| AppError::Internal(format!("Failed to encode JWT: {}", e)))?;

        // Generate redirect URL
        let frontend_host = keypair.frontend_host;

        let redirect_url = format!(
            "https://{}?impersonation_token={}",
            frontend_host,
            urlencoding::encode(&token)
        );

        Ok(GenerateImpersonationTokenResponse {
            token,
            redirect_url,
        })
    }
}
