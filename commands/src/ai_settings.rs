use common::{
    EncryptionService, HasDbRouter, HasEncryptionProvider, ensure_knowledge_base_indices,
    error::AppError, initialize_memory_table,
};
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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<DeploymentAiSettings, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            INSERT INTO deployment_ai_settings (deployment_id)
            VALUES ($1)
            RETURNING
                id,
                deployment_id,
                strong_llm_provider,
                weak_llm_provider,
                gemini_api_key,
                openrouter_api_key,
                openrouter_require_parameters,
                openai_api_key,
                anthropic_api_key,
                strong_model,
                weak_model,
                storage_provider,
                storage_bucket,
                storage_region,
                storage_endpoint,
                storage_root_prefix,
                storage_force_path_style,
                storage_access_key_id,
                storage_secret_access_key,
                vector_store_initialized_at,
                created_at,
                updated_at
            "#,
        )
        .bind(self.deployment_id)
        .fetch_one(executor)
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
        D: HasDbRouter + HasEncryptionProvider,
    {
        let writer = deps.db_router().writer();
        let encryptor = deps.encryption_provider();
        let encrypted_updates = encrypt_ai_settings_updates(&self.updates, encryptor)?;

        let storage_updates = self.updates.storage.as_ref();
        let storage_provider = storage_updates
            .and_then(|storage| storage.provider.as_ref())
            .map(|_| "s3".to_string());
        let storage_bucket = storage_updates.and_then(|storage| storage.bucket.clone());
        let storage_region = storage_updates.and_then(|storage| storage.region.clone());
        let storage_endpoint = storage_updates.and_then(|storage| storage.endpoint.clone());
        let storage_root_prefix = storage_updates.and_then(|storage| storage.root_prefix.clone());
        let storage_force_path_style = storage_updates.and_then(|storage| storage.force_path_style);

        let mut result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            UPDATE deployment_ai_settings SET
                strong_llm_provider = COALESCE($2, strong_llm_provider),
                weak_llm_provider = COALESCE($3, weak_llm_provider),
                gemini_api_key = COALESCE($4, gemini_api_key),
                openrouter_api_key = COALESCE($5, openrouter_api_key),
                openrouter_require_parameters = COALESCE($6, openrouter_require_parameters),
                openai_api_key = COALESCE($7, openai_api_key),
                anthropic_api_key = COALESCE($8, anthropic_api_key),
                strong_model = COALESCE($9, strong_model),
                weak_model = COALESCE($10, weak_model),
                storage_provider = COALESCE($11, storage_provider),
                storage_bucket = COALESCE($12, storage_bucket),
                storage_region = COALESCE($13, storage_region),
                storage_endpoint = COALESCE($14, storage_endpoint),
                storage_root_prefix = COALESCE($15, storage_root_prefix),
                storage_force_path_style = COALESCE($16, storage_force_path_style),
                storage_access_key_id = COALESCE($17, storage_access_key_id),
                storage_secret_access_key = COALESCE($18, storage_secret_access_key),
                vector_store_initialized_at = CASE
                    WHEN $19::boolean THEN NULL
                    ELSE vector_store_initialized_at
                END,
                updated_at = NOW()
            WHERE deployment_id = $1
            RETURNING
                id,
                deployment_id,
                strong_llm_provider,
                weak_llm_provider,
                gemini_api_key,
                openrouter_api_key,
                openrouter_require_parameters,
                openai_api_key,
                anthropic_api_key,
                strong_model,
                weak_model,
                storage_provider,
                storage_bucket,
                storage_region,
                storage_endpoint,
                storage_root_prefix,
                storage_force_path_style,
                storage_access_key_id,
                storage_secret_access_key,
                vector_store_initialized_at,
                created_at,
                updated_at
            "#,
        )
        .bind(self.deployment_id)
        .bind(&encrypted_updates.strong_llm_provider)
        .bind(&encrypted_updates.weak_llm_provider)
        .bind(&encrypted_updates.gemini_api_key)
        .bind(&encrypted_updates.openrouter_api_key)
        .bind(encrypted_updates.openrouter_require_parameters)
        .bind(&encrypted_updates.openai_api_key)
        .bind(&encrypted_updates.anthropic_api_key)
        .bind(&encrypted_updates.strong_model)
        .bind(&encrypted_updates.weak_model)
        .bind(&storage_provider)
        .bind(&storage_bucket)
        .bind(&storage_region)
        .bind(&storage_endpoint)
        .bind(&storage_root_prefix)
        .bind(storage_force_path_style)
        .bind(&encrypted_updates.storage_access_key_id)
        .bind(&encrypted_updates.storage_secret_access_key)
        .bind(storage_updates.is_some())
        .fetch_one(writer)
        .await?;

        if storage_updates.is_some() {
            initialize_vector_stores(&result, deps).await?;
            result = sqlx::query_as::<_, DeploymentAiSettings>(
                r#"
                UPDATE deployment_ai_settings SET
                    vector_store_initialized_at = NOW(),
                    updated_at = NOW()
                WHERE deployment_id = $1
                RETURNING
                    id,
                    deployment_id,
                    strong_llm_provider,
                    weak_llm_provider,
                    gemini_api_key,
                    openrouter_api_key,
                    openrouter_require_parameters,
                    openai_api_key,
                    anthropic_api_key,
                    strong_model,
                    weak_model,
                    storage_provider,
                    storage_bucket,
                    storage_region,
                    storage_endpoint,
                    storage_root_prefix,
                    storage_force_path_style,
                    storage_access_key_id,
                    storage_secret_access_key,
                    vector_store_initialized_at,
                    created_at,
                    updated_at
                "#,
            )
            .bind(self.deployment_id)
            .fetch_one(writer)
            .await?;
        }

        Ok(result)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
struct EncryptedAiSettingsUpdate {
    strong_llm_provider: Option<String>,
    weak_llm_provider: Option<String>,
    gemini_api_key: Option<String>,
    openrouter_api_key: Option<String>,
    openrouter_require_parameters: Option<bool>,
    openai_api_key: Option<String>,
    anthropic_api_key: Option<String>,
    strong_model: Option<String>,
    weak_model: Option<String>,
    storage_access_key_id: Option<String>,
    storage_secret_access_key: Option<String>,
}

fn encrypt_ai_settings_updates(
    updates: &UpdateDeploymentAiSettingsRequest,
    encryptor: &dyn AiSettingsEncryptor,
) -> Result<EncryptedAiSettingsUpdate, AppError> {
    Ok(EncryptedAiSettingsUpdate {
        strong_llm_provider: updates
            .strong_llm_provider
            .as_ref()
            .map(|value| match value {
                models::DeploymentLlmProvider::Gemini => "gemini".to_string(),
                models::DeploymentLlmProvider::Openai => "openai".to_string(),
                models::DeploymentLlmProvider::Openrouter => "openrouter".to_string(),
            }),
        weak_llm_provider: updates.weak_llm_provider.as_ref().map(|value| match value {
            models::DeploymentLlmProvider::Gemini => "gemini".to_string(),
            models::DeploymentLlmProvider::Openai => "openai".to_string(),
            models::DeploymentLlmProvider::Openrouter => "openrouter".to_string(),
        }),
        gemini_api_key: updates
            .gemini_api_key
            .as_ref()
            .map(|value| encryptor.encrypt(value))
            .transpose()?,
        openrouter_api_key: updates
            .openrouter_api_key
            .as_ref()
            .map(|value| encryptor.encrypt(value))
            .transpose()?,
        openrouter_require_parameters: updates.openrouter_require_parameters,
        openai_api_key: updates
            .openai_api_key
            .as_ref()
            .map(|value| encryptor.encrypt(value))
            .transpose()?,
        anthropic_api_key: updates
            .anthropic_api_key
            .as_ref()
            .map(|value| encryptor.encrypt(value))
            .transpose()?,
        strong_model: updates.strong_model.clone(),
        weak_model: updates.weak_model.clone(),
        storage_access_key_id: updates
            .storage
            .as_ref()
            .and_then(|storage| storage.access_key_id.as_ref())
            .map(|value| encryptor.encrypt(value))
            .transpose()?,
        storage_secret_access_key: updates
            .storage
            .as_ref()
            .and_then(|storage| storage.secret_access_key.as_ref())
            .map(|value| encryptor.encrypt(value))
            .transpose()?,
    })
}

async fn initialize_vector_stores<D>(
    settings: &DeploymentAiSettings,
    deps: &D,
) -> Result<(), AppError>
where
    D: HasDbRouter + HasEncryptionProvider,
{
    let storage = crate::ResolveDeploymentStorageCommand::new(settings.deployment_id)
        .execute_with_deps(deps)
        .await?;
    let lance_config = storage.vector_store_config();

    ensure_knowledge_base_indices(&lance_config)
        .await
        .map_err(|error| {
            AppError::Internal(format!(
                "Knowledge base LanceDB initialization failed for {}: {}",
                lance_config.uri, error
            ))
        })?;

    initialize_memory_table(&lance_config)
        .await
        .map_err(|error| {
            AppError::Internal(format!(
                "Memory LanceDB initialization failed for {}: {}",
                lance_config.uri, error
            ))
        })?;

    Ok(())
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
    OpenRouter,
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
    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<(), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let column = match self.key_type {
            AiKeyType::Gemini => "gemini_api_key",
            AiKeyType::OpenRouter => "openrouter_api_key",
            AiKeyType::OpenAI => "openai_api_key",
            AiKeyType::Anthropic => "anthropic_api_key",
        };

        let query = format!(
            "UPDATE deployment_ai_settings SET {} = NULL, updated_at = NOW() WHERE deployment_id = $1",
            column
        );

        sqlx::query(&query)
            .bind(self.deployment_id)
            .execute(executor)
            .await?;

        Ok(())
    }
}
