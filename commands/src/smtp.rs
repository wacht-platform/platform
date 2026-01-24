use crate::Command;
use common::error::AppError;
use common::smtp::{SmtpConfig, SmtpService};
use common::state::AppState;
use models::{CustomSmtpConfig, EmailProvider};

pub struct VerifySmtpConnectionCommand {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_email: String,
    pub use_tls: bool,
}

impl VerifySmtpConnectionCommand {
    pub fn new(
        host: String,
        port: u16,
        username: String,
        password: String,
        from_email: String,
        use_tls: bool,
    ) -> Self {
        Self {
            host,
            port,
            username,
            password,
            from_email,
            use_tls,
        }
    }
}

impl Command for VerifySmtpConnectionCommand {
    type Output = ();

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        let config = SmtpConfig {
            host: self.host,
            port: self.port,
            username: self.username,
            password: self.password,
            from_email: self.from_email,
            use_tls: self.use_tls,
        };

        let smtp_service = SmtpService::new(config);
        smtp_service.test_connection().await?;

        Ok(())
    }
}

pub struct UpdateDeploymentSmtpConfigCommand {
    pub deployment_id: i64,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from_email: String,
    pub use_tls: bool,
}

impl UpdateDeploymentSmtpConfigCommand {
    pub fn new(
        deployment_id: i64,
        host: String,
        port: u16,
        username: String,
        password: String,
        from_email: String,
        use_tls: bool,
    ) -> Self {
        Self {
            deployment_id,
            host,
            port,
            username,
            password,
            from_email,
            use_tls,
        }
    }
}

impl Command for UpdateDeploymentSmtpConfigCommand {
    type Output = CustomSmtpConfig;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let encrypted_password = app_state.encryption_service.encrypt(&self.password)?;

        let config = CustomSmtpConfig {
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: encrypted_password.clone(),
            from_email: self.from_email.clone(),
            use_tls: self.use_tls,
            verified: true,
        };

        let mut config_json =
            serde_json::to_value(&config).map_err(|e| AppError::Serialization(e.to_string()))?;

        if let Some(obj) = config_json.as_object_mut() {
            obj.insert(
                "password".to_string(),
                serde_json::Value::String(encrypted_password.clone()),
            );
        }

        sqlx::query!(
            r#"
            UPDATE deployments
            SET email_provider = $1,
                custom_smtp_config = $2,
                updated_at = NOW()
            WHERE id = $3
            "#,
            EmailProvider::CustomSmtp.to_string(),
            config_json,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        tracing::info!(
            "Updated SMTP config for deployment {}: {}:{}",
            self.deployment_id,
            self.host,
            self.port
        );

        Ok(CustomSmtpConfig {
            host: self.host,
            port: self.port,
            username: self.username,
            password: String::new(),
            from_email: self.from_email,
            use_tls: self.use_tls,
            verified: true,
        })
    }
}

pub struct RemoveDeploymentSmtpConfigCommand {
    pub deployment_id: i64,
}

impl RemoveDeploymentSmtpConfigCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl Command for RemoveDeploymentSmtpConfigCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        sqlx::query!(
            r#"
            UPDATE deployments
            SET email_provider = $1,
                custom_smtp_config = NULL,
                updated_at = NOW()
            WHERE id = $2
            "#,
            EmailProvider::Postmark.to_string(),
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await?;

        crate::ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute(app_state)
            .await?;

        tracing::info!(
            "Removed SMTP config for deployment {}, reverted to Postmark",
            self.deployment_id
        );

        Ok(())
    }
}