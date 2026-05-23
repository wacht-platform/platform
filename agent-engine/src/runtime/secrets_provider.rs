use async_trait::async_trait;
use common::error::AppError;
use models::{DeploymentAiProviderProfile, DeploymentAiSettings};

use super::thread_execution_context::DeploymentProviderKeys;

#[async_trait]
pub trait SecretsProvider: Send + Sync {
    async fn resolve_provider_keys(
        &self,
        settings: Option<&DeploymentAiSettings>,
        profiles: &[DeploymentAiProviderProfile],
    ) -> Result<DeploymentProviderKeys, AppError>;
}

#[derive(Clone)]
pub struct SettingsSecretsProvider {
    encryption_service: common::EncryptionService,
}

impl SettingsSecretsProvider {
    pub fn new(encryption_service: common::EncryptionService) -> Self {
        Self { encryption_service }
    }
}

#[async_trait]
impl SecretsProvider for SettingsSecretsProvider {
    async fn resolve_provider_keys(
        &self,
        settings: Option<&DeploymentAiSettings>,
        profiles: &[DeploymentAiProviderProfile],
    ) -> Result<DeploymentProviderKeys, AppError> {
        DeploymentProviderKeys::from_settings(settings, profiles, &self.encryption_service)
    }
}
