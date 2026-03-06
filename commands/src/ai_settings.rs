use common::{EncryptionService, HasDbRouter, HasEncryptionService, error::AppError};
use models::{DeploymentAiSettings, UpdateDeploymentAiSettingsRequest};

pub trait AiSettingsEncryptor: Send + Sync {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError>;
}

impl AiSettingsEncryptor for EncryptionService {
    fn encrypt(&self, plaintext: &str) -> Result<String, AppError> {
        EncryptionService::encrypt(self, plaintext)
    }
}

/// Command to create initial AI settings for a new deployment
pub struct CreateDeploymentAiSettingsCommand {
    deployment_id: i64,
}

#[derive(Default)]
pub struct CreateDeploymentAiSettingsCommandBuilder {
    deployment_id: Option<i64>,
}

impl CreateDeploymentAiSettingsCommand {
    pub fn builder() -> CreateDeploymentAiSettingsCommandBuilder {
        CreateDeploymentAiSettingsCommandBuilder::default()
    }

    pub fn new(deployment_id: i64) -> Self {
        Self { deployment_id }
    }
}

impl CreateDeploymentAiSettingsCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<DeploymentAiSettings, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<DeploymentAiSettings, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            INSERT INTO deployment_ai_settings (deployment_id)
            VALUES ($1)
            RETURNING id, deployment_id, gemini_api_key, openai_api_key, anthropic_api_key, created_at, updated_at
            "#,
        )
        .bind(self.deployment_id)
        .fetch_one(&mut *conn)
        .await?;

        Ok(result)
    }
}

impl CreateDeploymentAiSettingsCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn build(self) -> Result<CreateDeploymentAiSettingsCommand, AppError> {
        Ok(CreateDeploymentAiSettingsCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
        })
    }
}

/// Command to update deployment AI settings (simple update, not upsert)
pub struct UpdateDeploymentAiSettingsCommand {
    deployment_id: i64,
    updates: UpdateDeploymentAiSettingsRequest,
}

#[derive(Default)]
pub struct UpdateDeploymentAiSettingsCommandBuilder {
    deployment_id: Option<i64>,
    updates: Option<UpdateDeploymentAiSettingsRequest>,
}

impl UpdateDeploymentAiSettingsCommand {
    pub fn builder() -> UpdateDeploymentAiSettingsCommandBuilder {
        UpdateDeploymentAiSettingsCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, updates: UpdateDeploymentAiSettingsRequest) -> Self {
        Self {
            deployment_id,
            updates,
        }
    }
}

impl UpdateDeploymentAiSettingsCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn updates(mut self, updates: UpdateDeploymentAiSettingsRequest) -> Self {
        self.updates = Some(updates);
        self
    }

    pub fn build(self) -> Result<UpdateDeploymentAiSettingsCommand, AppError> {
        Ok(UpdateDeploymentAiSettingsCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            updates: self
                .updates
                .ok_or_else(|| AppError::Validation("updates are required".to_string()))?,
        })
    }
}

impl UpdateDeploymentAiSettingsCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<DeploymentAiSettings, AppError>
    where
        D: HasDbRouter + HasEncryptionService,
    {
        self.execute_with(deps.db_router().writer(), deps.encryption_service())
            .await
    }

    pub async fn execute_with<'a, A>(
        self,
        acquirer: A,
        encryptor: &dyn AiSettingsEncryptor,
    ) -> Result<DeploymentAiSettings, AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.apply_with_conn(conn, encryptor).await
    }

    async fn apply_with_conn<C>(
        self,
        mut conn: C,
        encryptor: &dyn AiSettingsEncryptor,
    ) -> Result<DeploymentAiSettings, AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        // Encrypt API keys before storing
        let encrypted_gemini = self
            .updates
            .gemini_api_key
            .as_ref()
            .map(|k| encryptor.encrypt(k))
            .transpose()?;

        let encrypted_openai = self
            .updates
            .openai_api_key
            .as_ref()
            .map(|k| encryptor.encrypt(k))
            .transpose()?;

        let encrypted_anthropic = self
            .updates
            .anthropic_api_key
            .as_ref()
            .map(|k| encryptor.encrypt(k))
            .transpose()?;

        let result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            UPDATE deployment_ai_settings SET
                gemini_api_key = COALESCE($2, gemini_api_key),
                openai_api_key = COALESCE($3, openai_api_key),
                anthropic_api_key = COALESCE($4, anthropic_api_key),
                updated_at = NOW()
            WHERE deployment_id = $1
            RETURNING id, deployment_id, gemini_api_key, openai_api_key, anthropic_api_key, created_at, updated_at
            "#,
        )
        .bind(self.deployment_id)
        .bind(&encrypted_gemini)
        .bind(&encrypted_openai)
        .bind(&encrypted_anthropic)
        .fetch_one(&mut *conn)
        .await?;

        Ok(result)
    }
}

/// Command to clear a specific API key from deployment AI settings
pub struct ClearDeploymentAiKeyCommand {
    deployment_id: i64,
    key_type: AiKeyType,
}

#[derive(Default)]
pub struct ClearDeploymentAiKeyCommandBuilder {
    deployment_id: Option<i64>,
    key_type: Option<AiKeyType>,
}

impl ClearDeploymentAiKeyCommand {
    pub fn builder() -> ClearDeploymentAiKeyCommandBuilder {
        ClearDeploymentAiKeyCommandBuilder::default()
    }
}

pub enum AiKeyType {
    Gemini,
    OpenAI,
    Anthropic,
}

impl ClearDeploymentAiKeyCommand {
    pub fn new(deployment_id: i64, key_type: AiKeyType) -> Self {
        Self {
            deployment_id,
            key_type,
        }
    }
}

impl ClearDeploymentAiKeyCommandBuilder {
    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn key_type(mut self, key_type: AiKeyType) -> Self {
        self.key_type = Some(key_type);
        self
    }

    pub fn build(self) -> Result<ClearDeploymentAiKeyCommand, AppError> {
        Ok(ClearDeploymentAiKeyCommand {
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            key_type: self
                .key_type
                .ok_or_else(|| AppError::Validation("key_type is required".to_string()))?,
        })
    }
}

impl ClearDeploymentAiKeyCommand {
    pub async fn execute_with<'a, A>(self, acquirer: A) -> Result<(), AppError>
    where
        A: sqlx::Acquire<'a, Database = sqlx::Postgres>,
    {
        let conn = acquirer.acquire().await?;
        self.execute_with_deps(conn).await
    }

    async fn execute_with_deps<C>(self, mut conn: C) -> Result<(), AppError>
    where
        C: std::ops::DerefMut<Target = sqlx::PgConnection>,
    {
        let column = match self.key_type {
            AiKeyType::Gemini => "gemini_api_key",
            AiKeyType::OpenAI => "openai_api_key",
            AiKeyType::Anthropic => "anthropic_api_key",
        };

        let query = format!(
            "UPDATE deployment_ai_settings SET {} = NULL, updated_at = NOW() WHERE deployment_id = $1",
            column
        );

        sqlx::query(&query)
            .bind(self.deployment_id)
            .execute(&mut *conn)
            .await?;

        Ok(())
    }
}
