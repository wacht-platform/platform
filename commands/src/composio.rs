use common::{HasDbRouter, HasEncryptionProvider, error::AppError};
use models::UpdateComposioConfigRequest;

pub struct UpdateComposioConfigCommand {
    deployment_id: i64,
    updates: UpdateComposioConfigRequest,
}

impl UpdateComposioConfigCommand {
    pub fn new(deployment_id: i64, updates: UpdateComposioConfigRequest) -> Self {
        Self {
            deployment_id,
            updates,
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasEncryptionProvider,
    {
        let writer = deps.db_router().writer();
        let encryptor = deps.encryption_provider();

        let (set_api_key, api_key_value) = match self.updates.api_key {
            Some(Some(raw)) if !raw.trim().is_empty() => {
                let encrypted = encryptor.encrypt(raw.trim())?;
                (true, Some(encrypted))
            }
            Some(Some(_)) | Some(None) => (true, None),
            None => (false, None),
        };

        let enabled_apps_json = match self.updates.enabled_apps {
            Some(apps) => Some(serde_json::to_value(apps).map_err(|e| {
                AppError::Internal(format!("failed to serialize enabled_apps: {e}"))
            })?),
            None => None,
        };

        sqlx::query!(
            r#"
            UPDATE deployment_ai_settings SET
                composio_enabled = COALESCE($2, composio_enabled),
                composio_use_platform_key = COALESCE($3, composio_use_platform_key),
                composio_api_key = CASE
                    WHEN $4::boolean THEN $5
                    ELSE composio_api_key
                END,
                composio_enabled_apps = COALESCE($6, composio_enabled_apps),
                updated_at = NOW()
            WHERE deployment_id = $1
            "#,
            self.deployment_id,
            self.updates.enabled,
            self.updates.use_platform_key,
            set_api_key,
            api_key_value,
            enabled_apps_json,
        )
        .execute(writer)
        .await?;

        Ok(())
    }
}
