use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::{DeploymentAiSettings, UpdateDeploymentAiSettingsRequest};

/// Command to create initial AI settings for a new deployment
pub struct CreateDeploymentAiSettingsCommand {
    pub deployment_id: i64,
}

impl Command for CreateDeploymentAiSettingsCommand {
    type Output = DeploymentAiSettings;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let result = sqlx::query_as::<_, DeploymentAiSettings>(
            r#"
            INSERT INTO deployment_ai_settings (deployment_id)
            VALUES ($1)
            RETURNING id, deployment_id, gemini_api_key, openai_api_key, anthropic_api_key, created_at, updated_at
            "#,
        )
        .bind(self.deployment_id)
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(result)
    }
}

/// Command to update deployment AI settings (simple update, not upsert)
pub struct UpdateDeploymentAiSettingsCommand {
    pub deployment_id: i64,
    pub updates: UpdateDeploymentAiSettingsRequest,
}

impl UpdateDeploymentAiSettingsCommand {
    pub fn new(deployment_id: i64, updates: UpdateDeploymentAiSettingsRequest) -> Self {
        Self {
            deployment_id,
            updates,
        }
    }
}

impl Command for UpdateDeploymentAiSettingsCommand {
    type Output = DeploymentAiSettings;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Encrypt API keys before storing
        let encrypted_gemini = self
            .updates
            .gemini_api_key
            .as_ref()
            .map(|k| app_state.encryption_service.encrypt(k))
            .transpose()?;

        let encrypted_openai = self
            .updates
            .openai_api_key
            .as_ref()
            .map(|k| app_state.encryption_service.encrypt(k))
            .transpose()?;

        let encrypted_anthropic = self
            .updates
            .anthropic_api_key
            .as_ref()
            .map(|k| app_state.encryption_service.encrypt(k))
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
        .fetch_one(&app_state.db_pool)
        .await?;

        Ok(result)
    }
}

/// Command to clear a specific API key from deployment AI settings
pub struct ClearDeploymentAiKeyCommand {
    pub deployment_id: i64,
    pub key_type: AiKeyType,
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

impl Command for ClearDeploymentAiKeyCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
            .execute(&app_state.db_pool)
            .await?;

        Ok(())
    }
}
