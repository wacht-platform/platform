use chrono::{Duration, Utc};

use crate::{Command, SendEmailCommand};
use common::db_router::ReadConsistency;
use common::error::AppError;
use common::state::AppState;
use dto::json::InviteUserRequest;
use models::DeploymentInvitation;
use sqlx::Connection;

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

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        app_state: &AppState,
        invitation_id: i64,
    ) -> Result<DeploymentInvitation, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let now = Utc::now();
        let expiry_days = self.request.expiry_days.unwrap_or(7);
        let expiry = now + Duration::days(expiry_days);

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
        .execute(&mut *conn)
        .await?;

        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        let deployment_settings =
            queries::deployment::GetDeploymentWithSettingsQuery::new(self.deployment_id)
                .execute_with(reader)
                .await
                .map_err(|e| {
                    AppError::Internal(format!("Failed to fetch deployment settings: {}", e))
                })?;

        let app_name = deployment_settings
            .ui_settings
            .as_ref()
            .map(|ui| ui.app_name.clone())
            .unwrap_or_else(|| "".to_string());

        let app_logo_url = deployment_settings
            .ui_settings
            .as_ref()
            .map(|ui| ui.logo_image_url.clone());

        let variables = serde_json::json!({
            "app": {
                "name": app_name,
                "logo": app_logo_url
            },
            "user": {
                "first_name": self.request.first_name.clone(),
                "last_name": self.request.last_name.clone()
            },
            "invitation": {
                "expires_in_days": expiry_days.to_string()
            },
            "action_url": format!("https://{}/sign-up?invite_token={}", deployment_settings.frontend_host, token)
        });

        let send_email_command = SendEmailCommand::new(
            self.deployment_id,
            "waitlist_invite_template".to_string(),
            self.request.email_address.clone(),
            variables,
        );
        Command::execute(send_email_command, app_state).await?;

        Ok(DeploymentInvitation {
            id: invitation_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            first_name: self.request.first_name,
            last_name: self.request.last_name,
            email_address: self.request.email_address,
            token,
            expiry,
        })
    }
}

impl Command for InviteUserCommand {
    type Output = DeploymentInvitation;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(
            &app_state.db_pool,
            app_state,
            app_state.sf.next_id()? as i64,
        )
        .await
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

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        app_state: &AppState,
        invitation_id: i64,
    ) -> Result<DeploymentInvitation, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let now = Utc::now();
        let mut tx = conn.begin().await?;

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

        let expiry = now + Duration::days(7);
        let first_name = waitlist_user.first_name.unwrap_or_default();
        let last_name = waitlist_user.last_name.unwrap_or_default();
        let email_address = waitlist_user.email_address.unwrap_or_default();

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

        let reader = app_state.db_router.reader(ReadConsistency::Strong);
        let deployment_settings =
            queries::deployment::GetDeploymentWithSettingsQuery::new(self.deployment_id)
                .execute_with(reader)
                .await
                .map_err(|e| {
                    AppError::Internal(format!("Failed to fetch deployment settings: {}", e))
                })?;

        let app_name = deployment_settings
            .ui_settings
            .as_ref()
            .map(|ui| ui.app_name.clone())
            .unwrap_or_else(|| "".to_string());

        let app_logo_url = deployment_settings
            .ui_settings
            .as_ref()
            .map(|ui| ui.logo_image_url.clone());

        let variables = serde_json::json!({
            "app": {
                "name": app_name,
                "logo": app_logo_url
            },
            "action_url": format!("https://{}/sign-up?invite_token={}", deployment_settings.frontend_host, token)
        });

        let send_email_command = SendEmailCommand::new(
            self.deployment_id,
            "waitlist_invite_template".to_string(),
            email_address.clone(),
            variables,
        );
        Command::execute(send_email_command, app_state).await?;

        sqlx::query!(
            "DELETE FROM deployment_waitlist_users WHERE id = $1",
            self.waitlist_user_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(DeploymentInvitation {
            id: invitation_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            first_name: first_name.clone(),
            last_name: last_name.clone(),
            email_address: email_address.clone(),
            token,
            expiry,
        })
    }
}

impl Command for ApproveWaitlistUserCommand {
    type Output = DeploymentInvitation;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(
            &app_state.db_pool,
            app_state,
            app_state.sf.next_id()? as i64,
        )
        .await
    }
}

pub struct DeleteInvitationCommand {
    deployment_id: i64,
    invitation_id: i64,
}

impl DeleteInvitationCommand {
    pub fn new(deployment_id: i64, invitation_id: i64) -> Self {
        Self {
            deployment_id,
            invitation_id,
        }
    }
}

impl Command for DeleteInvitationCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(&app_state.db_pool).await
    }
}

impl DeleteInvitationCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let result = sqlx::query!(
            r#"
            DELETE FROM deployment_invitations
            WHERE id = $1 AND deployment_id = $2
            "#,
            self.invitation_id,
            self.deployment_id
        )
        .execute(&mut *conn)
        .await?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Invitation not found".to_string()));
        }

        Ok(())
    }
}
