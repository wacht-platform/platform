use commands::{
    PendingDeploymentStorageConfig, UpdateDeploymentAiSettingsCommand,
    test_deployment_storage_connection,
};
use common::HasEncryptionProvider;
use common::db_router::ReadConsistency;
use common::error::AppError;
use models::{
    DeploymentAiSettings, DeploymentAiSettingsResponse, DeploymentLlmProvider,
    DeploymentStorageProvider, DeploymentStorageSettingsResponse,
    UpdateDeploymentAiSettingsRequest, UpdateDeploymentStorageSettingsRequest,
};
use queries::GetDeploymentAiSettingsQuery;

use crate::application::AppState;
use common::deps;

pub async fn get_ai_settings(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentAiSettingsResponse, AppError> {
    let settings = GetDeploymentAiSettingsQuery::builder()
        .deployment_id(deployment_id)
        .build()?
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(match settings {
        Some(settings) => DeploymentAiSettingsResponse::from(settings),
        None => DeploymentAiSettingsResponse {
            strong_llm_provider: DeploymentLlmProvider::Gemini,
            weak_llm_provider: DeploymentLlmProvider::Gemini,
            gemini_api_key_set: false,
            openrouter_api_key_set: false,
            openrouter_require_parameters: true,
            openai_api_key_set: false,
            anthropic_api_key_set: false,
            strong_model: None,
            weak_model: None,
            storage: DeploymentStorageSettingsResponse {
                provider: DeploymentStorageProvider::S3,
                bucket: None,
                region: None,
                endpoint: None,
                root_prefix: None,
                force_path_style: false,
                access_key_id_set: false,
                secret_access_key_set: false,
            },
        },
    })
}

pub async fn update_ai_settings(
    app_state: &AppState,
    deployment_id: i64,
    updates: UpdateDeploymentAiSettingsRequest,
) -> Result<DeploymentAiSettingsResponse, AppError> {
    let existing_settings = GetDeploymentAiSettingsQuery::builder()
        .deployment_id(deployment_id)
        .build()?
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let normalized_updates = normalize_ai_settings_updates(updates);
    validate_storage_settings(existing_settings.as_ref(), &normalized_updates)?;

    let deps = deps::from_app(app_state).db().enc();
    if let Some(storage_updates) = normalized_updates.storage.as_ref() {
        let connection_test_config =
            merged_storage_connection_config(existing_settings.as_ref(), storage_updates, &deps)?;
        let probe_key = format!(
            "__wacht_storage_check/{}/{}.txt",
            deployment_id,
            app_state.sf.next_id()? as i64
        );
        let probe_body = format!("wacht-storage-check:{}", probe_key).into_bytes();

        test_deployment_storage_connection(&connection_test_config, &probe_key, &probe_body)
            .await
            .map_err(|e| {
                AppError::Validation(format!(
                    "Unable to verify customer S3 storage settings: {}",
                    e
                ))
            })?;
    }

    let settings = UpdateDeploymentAiSettingsCommand::builder()
        .deployment_id(deployment_id)
        .updates(normalized_updates)
        .build()?
        .execute_with_deps(&deps)
        .await?;

    Ok(DeploymentAiSettingsResponse::from(settings))
}

fn normalize_ai_settings_updates(
    updates: UpdateDeploymentAiSettingsRequest,
) -> UpdateDeploymentAiSettingsRequest {
    UpdateDeploymentAiSettingsRequest {
        strong_llm_provider: updates.strong_llm_provider,
        weak_llm_provider: updates.weak_llm_provider,
        gemini_api_key: normalize_optional_text(updates.gemini_api_key),
        openrouter_api_key: normalize_optional_text(updates.openrouter_api_key),
        openrouter_require_parameters: updates.openrouter_require_parameters,
        openai_api_key: normalize_optional_text(updates.openai_api_key),
        anthropic_api_key: normalize_optional_text(updates.anthropic_api_key),
        strong_model: normalize_optional_text(updates.strong_model),
        weak_model: normalize_optional_text(updates.weak_model),
        storage: updates.storage.and_then(normalize_storage_settings_updates),
    }
}

fn normalize_storage_settings_updates(
    updates: UpdateDeploymentStorageSettingsRequest,
) -> Option<UpdateDeploymentStorageSettingsRequest> {
    let normalized = UpdateDeploymentStorageSettingsRequest {
        provider: Some(updates.provider.unwrap_or_default()),
        bucket: normalize_optional_text(updates.bucket),
        region: normalize_optional_text(updates.region),
        endpoint: normalize_optional_text(updates.endpoint),
        root_prefix: normalize_optional_text(updates.root_prefix),
        force_path_style: updates.force_path_style,
        access_key_id: normalize_optional_text(updates.access_key_id),
        secret_access_key: normalize_optional_text(updates.secret_access_key),
    };

    let has_updates = normalized.provider.is_some()
        || normalized.bucket.is_some()
        || normalized.region.is_some()
        || normalized.endpoint.is_some()
        || normalized.root_prefix.is_some()
        || normalized.force_path_style.is_some()
        || normalized.access_key_id.is_some()
        || normalized.secret_access_key.is_some();

    has_updates.then_some(normalized)
}

fn normalize_optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_storage_settings(
    existing_settings: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> Result<(), AppError> {
    if let Some(provider) = updates
        .storage
        .as_ref()
        .and_then(|storage| storage.provider.clone())
    {
        if provider != DeploymentStorageProvider::S3 {
            return Err(AppError::Validation(
                "storage.provider must be s3".to_string(),
            ));
        }
    }

    let bucket = merged_storage_text(
        existing_settings.and_then(|settings| settings.storage_bucket.as_deref()),
        updates
            .storage
            .as_ref()
            .and_then(|storage| storage.bucket.as_deref()),
    );
    let endpoint = merged_storage_text(
        existing_settings.and_then(|settings| settings.storage_endpoint.as_deref()),
        updates
            .storage
            .as_ref()
            .and_then(|storage| storage.endpoint.as_deref()),
    );

    if bucket.is_none() {
        return Err(AppError::Validation(
            "storage.bucket is required when storage.provider is s3".to_string(),
        ));
    }

    let Some(endpoint) = endpoint else {
        return Err(AppError::Validation(
            "storage.endpoint is required when storage.provider is s3".to_string(),
        ));
    };

    let parsed_endpoint = url::Url::parse(endpoint)
        .map_err(|_| AppError::Validation("storage.endpoint must be a valid URL".to_string()))?;
    if !matches!(parsed_endpoint.scheme(), "http" | "https") {
        return Err(AppError::Validation(
            "storage.endpoint must use http or https".to_string(),
        ));
    }

    let has_access_key_id = updates
        .storage
        .as_ref()
        .and_then(|storage| storage.access_key_id.as_ref())
        .is_some()
        || existing_settings
            .and_then(|settings| settings.storage_access_key_id.as_ref())
            .is_some();
    if !has_access_key_id {
        return Err(AppError::Validation(
            "storage.access_key_id is required when storage.provider is s3".to_string(),
        ));
    }

    let has_secret_access_key = updates
        .storage
        .as_ref()
        .and_then(|storage| storage.secret_access_key.as_ref())
        .is_some()
        || existing_settings
            .and_then(|settings| settings.storage_secret_access_key.as_ref())
            .is_some();
    if !has_secret_access_key {
        return Err(AppError::Validation(
            "storage.secret_access_key is required when storage.provider is s3".to_string(),
        ));
    }

    Ok(())
}

fn merged_storage_text<'a>(existing: Option<&'a str>, updated: Option<&'a str>) -> Option<&'a str> {
    updated
        .or(existing)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn merged_storage_connection_config(
    existing_settings: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentStorageSettingsRequest,
    deps: &impl HasEncryptionProvider,
) -> Result<PendingDeploymentStorageConfig, AppError> {
    let bucket = merged_storage_text(
        existing_settings.and_then(|settings| settings.storage_bucket.as_deref()),
        updates.bucket.as_deref(),
    )
    .ok_or_else(|| AppError::Validation("storage.bucket is required".to_string()))?
    .to_string();

    let endpoint = merged_storage_text(
        existing_settings.and_then(|settings| settings.storage_endpoint.as_deref()),
        updates.endpoint.as_deref(),
    )
    .ok_or_else(|| AppError::Validation("storage.endpoint is required".to_string()))?
    .to_string();

    let region = merged_storage_text(
        existing_settings.and_then(|settings| settings.storage_region.as_deref()),
        updates.region.as_deref(),
    )
    .unwrap_or("auto")
    .to_string();

    let root_prefix = merged_storage_text(
        existing_settings.and_then(|settings| settings.storage_root_prefix.as_deref()),
        updates.root_prefix.as_deref(),
    )
    .map(ToOwned::to_owned);

    let force_path_style = updates.force_path_style.unwrap_or_else(|| {
        existing_settings
            .map(|settings| settings.storage_force_path_style)
            .unwrap_or(false)
    });

    let access_key_id = match updates.access_key_id.clone() {
        Some(value) => value,
        None => decrypt_existing_storage_secret(
            "storage.access_key_id",
            existing_settings.and_then(|settings| settings.storage_access_key_id.as_deref()),
            deps,
        )?,
    };

    let secret_access_key = match updates.secret_access_key.clone() {
        Some(value) => value,
        None => decrypt_existing_storage_secret(
            "storage.secret_access_key",
            existing_settings.and_then(|settings| settings.storage_secret_access_key.as_deref()),
            deps,
        )?,
    };

    Ok(PendingDeploymentStorageConfig {
        bucket,
        endpoint,
        region,
        root_prefix,
        force_path_style,
        access_key_id,
        secret_access_key,
    })
}

fn decrypt_existing_storage_secret(
    field_name: &str,
    encrypted_value: Option<&str>,
    deps: &impl HasEncryptionProvider,
) -> Result<String, AppError> {
    let encrypted = encrypted_value.ok_or_else(|| {
        AppError::Validation(format!(
            "{field_name} is required when storage.provider is s3"
        ))
    })?;

    deps.encryption_provider()
        .decrypt(encrypted)
        .map_err(|error| AppError::Internal(format!("Failed to decrypt {field_name}: {error}")))
}
