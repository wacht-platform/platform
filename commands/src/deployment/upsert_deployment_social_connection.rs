use std::str::FromStr;

use chrono::Utc;

use crate::Command;
use common::error::AppError;
use common::state::AppState;
use dto::json::DeploymentSocialConnectionUpsert;
use models::{DeploymentSocialConnection, OauthCredentials, SocialConnectionProvider};

use super::ClearDeploymentCacheCommand;

pub struct UpsertDeploymentSocialConnectionCommand {
    pub deployment_id: i64,
    pub connection: DeploymentSocialConnectionUpsert,
}

impl UpsertDeploymentSocialConnectionCommand {
    pub fn new(deployment_id: i64, connection: DeploymentSocialConnectionUpsert) -> Self {
        Self {
            deployment_id,
            connection,
        }
    }
}

impl UpsertDeploymentSocialConnectionCommand {
    pub async fn execute_with(self, app_state: &AppState) -> Result<DeploymentSocialConnection, AppError> {
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
        .fetch_optional(&app_state.db_pool)
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
            app_state.sf.next_id()? as i64,
            Utc::now(),
            Utc::now(),
            self.deployment_id,
            provider.map(String::from),
            enabled,
            serde_json::to_value(credentials)
                .map_err(|e| AppError::Serialization(e.to_string()))?,
        )
        .fetch_one(&app_state.db_pool)
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
            .execute_with(app_state)
            .await?;

        Ok(connection)
    }
}

impl Command for UpsertDeploymentSocialConnectionCommand {
    type Output = DeploymentSocialConnection;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        self.execute_with(app_state).await
    }
}
