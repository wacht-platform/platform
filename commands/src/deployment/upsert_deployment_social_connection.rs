use std::str::FromStr;

use chrono::Utc;

use common::{HasDbRouter, HasRedis, error::AppError};
use dto::json::DeploymentSocialConnectionUpsert;
use models::{DeploymentSocialConnection, OauthCredentials, SocialConnectionProvider};

use super::ClearDeploymentCacheCommand;

pub struct UpsertDeploymentSocialConnectionCommand {
    social_connection_id: Option<i64>,
    deployment_id: i64,
    connection: DeploymentSocialConnectionUpsert,
}

#[derive(Default)]
pub struct UpsertDeploymentSocialConnectionCommandBuilder {
    social_connection_id: Option<i64>,
    deployment_id: Option<i64>,
    connection: Option<DeploymentSocialConnectionUpsert>,
}

impl UpsertDeploymentSocialConnectionCommand {
    pub fn builder() -> UpsertDeploymentSocialConnectionCommandBuilder {
        UpsertDeploymentSocialConnectionCommandBuilder::default()
    }

    pub fn new(deployment_id: i64, connection: DeploymentSocialConnectionUpsert) -> Self {
        Self {
            social_connection_id: None,
            deployment_id,
            connection,
        }
    }
}

impl UpsertDeploymentSocialConnectionCommand {
    pub async fn execute_with_deps<D>(
        self,
        deps: &D,
    ) -> Result<DeploymentSocialConnection, AppError>
    where
        D: HasDbRouter + HasRedis,
    {
        let writer = deps.db_router().writer();
        let social_connection_id = self
            .social_connection_id
            .ok_or_else(|| AppError::Validation("social_connection_id is required".to_string()))?;
        let provider = self.connection.provider;
        let enabled = self.connection.enabled;
        let mut credentials = self.connection.credentials;

        let deployment = sqlx::query!(
            r#"
            SELECT mode
            FROM deployments
            WHERE id = $1 AND deleted_at IS NULL
            "#,
            self.deployment_id
        )
        .fetch_optional(writer)
        .await?
        .ok_or_else(|| AppError::NotFound("Deployment not found".to_string()))?;

        let is_production = deployment.mode.eq_ignore_ascii_case("production");
        let is_enabling = enabled.unwrap_or(false);

        if is_production && is_enabling {
            let credentials_ref = credentials.as_ref().ok_or_else(|| {
                AppError::Validation(
                    "Custom credentials are required to enable social login in production"
                        .to_string(),
                )
            })?;

            if credentials_ref.client_id.trim().is_empty()
                || credentials_ref.client_secret.trim().is_empty()
                || credentials_ref.redirect_uri.trim().is_empty()
            {
                return Err(AppError::Validation(
                    "Custom credentials are required to enable social login in production"
                        .to_string(),
                ));
            }
        }

        if let (Some(provider_ref), Some(credentials_ref)) =
            (provider.as_ref(), credentials.as_mut())
        {
            if credentials_ref.scopes.is_empty() {
                credentials_ref.scopes = provider_ref.default_scopes();
            }
        }

        let result = sqlx::query!(
            r#"
            INSERT INTO deployment_social_connections (id, created_at, updated_at, deployment_id, provider, enabled, credentials)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (deployment_id, provider) DO UPDATE SET updated_at = NOW(), enabled = EXCLUDED.enabled, credentials = EXCLUDED.credentials RETURNING *
            "#,
            social_connection_id,
            Utc::now(),
            Utc::now(),
            self.deployment_id,
            provider.map(String::from),
            enabled,
            serde_json::to_value(credentials)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
        )
        .fetch_one(writer)
        .await?;

        let parsed_provider = result
            .provider
            .as_deref()
            .and_then(|provider| SocialConnectionProvider::from_str(provider).ok());

        let parsed_credentials = match result.credentials {
            Some(value) => serde_json::from_value::<Option<OauthCredentials>>(value)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
            None => None,
        };

        let connection = DeploymentSocialConnection {
            id: result.id,
            created_at: result.created_at,
            updated_at: result.updated_at,
            deployment_id: result.deployment_id,
            provider: parsed_provider,
            enabled: result.enabled,
            credentials: parsed_credentials,
        };

        ClearDeploymentCacheCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;

        Ok(connection)
    }
}

impl UpsertDeploymentSocialConnectionCommandBuilder {
    pub fn social_connection_id(mut self, social_connection_id: i64) -> Self {
        self.social_connection_id = Some(social_connection_id);
        self
    }

    pub fn deployment_id(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub fn connection(mut self, connection: DeploymentSocialConnectionUpsert) -> Self {
        self.connection = Some(connection);
        self
    }

    pub fn build(self) -> Result<UpsertDeploymentSocialConnectionCommand, AppError> {
        Ok(UpsertDeploymentSocialConnectionCommand {
            social_connection_id: self.social_connection_id,
            deployment_id: self
                .deployment_id
                .ok_or_else(|| AppError::Validation("deployment_id is required".to_string()))?,
            connection: self
                .connection
                .ok_or_else(|| AppError::Validation("connection is required".to_string()))?,
        })
    }
}
