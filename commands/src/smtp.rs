use crate::Command;
use common::EncryptionService;
use common::{HasDbRouter, HasEncryptionService, HasRedis, error::AppError};
use common::smtp::{SmtpConfig, SmtpService};
use common::state::AppState;
use models::{CustomSmtpConfig, EmailProvider};

pub trait SmtpConfigEncryptor: Send + Sync {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError>;
}

impl SmtpConfigEncryptor for EncryptionService {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        EncryptionService::encrypt(self, plaintext)
    }
}

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
        self.execute_with().await
    }
}

impl VerifySmtpConnectionCommand {
    pub async fn execute_with(self) -> Result<(), AppError> {
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
        self.execute_with_deps(app_state).await
    }
}

impl UpdateDeploymentSmtpConfigCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<CustomSmtpConfig, AppError>
    where
        D: HasDbRouter + HasEncryptionService,
    {
        self.execute_with(deps.db_router().writer(), deps.encryption_service())
            .await
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        encryptor: &dyn SmtpConfigEncryptor,
    ) -> Result<CustomSmtpConfig, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
        let encrypted_password = encryptor.encrypt(&self.password)?;

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
        .execute(&mut *conn)
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
        self.execute_with_deps(app_state).await
    }
}

impl RemoveDeploymentSmtpConfigCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasRedis,
    {
        self.execute_with(deps.db_router().writer(), deps.redis_client())
            .await
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        redis: &redis::Client,
    ) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let mut conn = acquirer.acquire().await?;
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
        .execute(&mut *conn)
        .await?;

        crate::ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with_deps(&mut conn, redis)
            .await?;

        tracing::info!(
            "Removed SMTP config for deployment {}, reverted to Postmark",
            self.deployment_id
        );

        Ok(())
    }
}
