use common::ResultExt;
use common::{EmbeddingApiProvider, HasEmbeddingProvider, HasEncryptionProvider, error::AppError};
use models::{
    DeploymentAiSettings, DeploymentEmbeddingProvider, DeploymentLlmProvider,
    UpdateDeploymentAiSettingsRequest, default_embedding_dimension,
    default_embedding_model_for_provider, default_embedding_provider,
    is_supported_embedding_dimension,
};

pub(super) async fn run_embedding_admission_if_needed(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
    deps: &(impl HasEncryptionProvider + HasEmbeddingProvider),
) -> Result<(), AppError> {
    if !embedding_admission_needed(existing, updates) {
        return Ok(());
    }

    let provider = effective_embedding_provider(existing, updates);
    let model = effective_embedding_model_for_provider(&provider, existing, updates);
    let dimension = effective_embedding_dimension(existing, updates)?;
    let api_key = effective_embedding_api_key_for_provider(&provider, existing, updates, deps)?;

    let embedding = deps
        .embedding_provider()
        .embed_content_with(
            embedding_provider_key(&provider),
            &model,
            "embedding compatibility probe".to_string(),
            Some(dimension),
            Some(api_key.as_str()),
        )
        .await
        .map_err(|e| {
            AppError::Validation(format!(
                "Embedding model admission failed — {}/{} cannot be saved. Details: {}",
                embedding_provider_name(&provider),
                model,
                friendly_admission_error(e),
            ))
        })?;

    if embedding.len() != dimension as usize {
        return Err(AppError::Validation(format!(
            "Embedding model admission failed — provider {} model {} returned {} dimensions, expected {}.",
            embedding_provider_name(&provider),
            model,
            embedding.len(),
            dimension
        )));
    }

    if embedding.iter().any(|value| !value.is_finite()) {
        return Err(AppError::Validation(format!(
            "Embedding model admission failed — provider {} model {} returned non-finite values.",
            embedding_provider_name(&provider),
            model
        )));
    }

    Ok(())
}

pub(super) fn validate_provider_key_consistency(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> Result<(), AppError> {
    if let Some(strong_provider) = updates.strong_llm_provider.as_ref() {
        if !key_available_for_llm_provider(strong_provider, existing, updates) {
            return Err(AppError::Validation(format!(
                "strong_llm_provider is set to {} but no {} API key is configured",
                llm_provider_name(strong_provider),
                llm_provider_name(strong_provider),
            )));
        }
    }

    if let Some(weak_provider) = updates.weak_llm_provider.as_ref() {
        if !key_available_for_llm_provider(weak_provider, existing, updates) {
            return Err(AppError::Validation(format!(
                "weak_llm_provider is set to {} but no {} API key is configured",
                llm_provider_name(weak_provider),
                llm_provider_name(weak_provider),
            )));
        }
    }

    Ok(())
}

pub(super) fn validate_embedding_provider_settings(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> Result<(), AppError> {
    let embedding_touched =
        updates.embedding_provider.is_some() || updates.embedding_model.is_some();
    if !embedding_touched {
        return Ok(());
    }

    if updates.embedding_provider.is_some() ^ updates.embedding_model.is_some() {
        return Err(AppError::Validation(
            "embedding_provider and embedding_model must be provided together".to_string(),
        ));
    }

    let provider = effective_embedding_provider(existing, updates);
    let model = effective_embedding_model_for_provider(&provider, existing, updates);

    if model.trim().is_empty() {
        return Err(AppError::Validation(
            "embedding_model cannot be empty".to_string(),
        ));
    }

    if !key_available_for_embedding_provider(&provider, existing, updates) {
        return Err(AppError::Validation(format!(
            "embedding_provider is set to {} but no {} API key is configured",
            embedding_provider_name(&provider),
            embedding_provider_name(&provider),
        )));
    }

    Ok(())
}

fn key_available_for_llm_provider(
    provider: &DeploymentLlmProvider,
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    match provider {
        DeploymentLlmProvider::Gemini => {
            updates.gemini_api_key.is_some()
                || existing.and_then(|e| e.gemini_api_key.as_ref()).is_some()
        }
        DeploymentLlmProvider::Openai => {
            updates.openai_api_key.is_some()
                || existing.and_then(|e| e.openai_api_key.as_ref()).is_some()
        }
        DeploymentLlmProvider::Openrouter => {
            updates.openrouter_api_key.is_some()
                || existing
                    .and_then(|e| e.openrouter_api_key.as_ref())
                    .is_some()
        }
    }
}

fn key_available_for_embedding_provider(
    provider: &DeploymentEmbeddingProvider,
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    match provider {
        DeploymentEmbeddingProvider::Gemini => {
            updates.gemini_api_key.is_some()
                || existing.and_then(|e| e.gemini_api_key.as_ref()).is_some()
        }
        DeploymentEmbeddingProvider::Openai => {
            updates.openai_api_key.is_some()
                || existing.and_then(|e| e.openai_api_key.as_ref()).is_some()
        }
        DeploymentEmbeddingProvider::Openrouter => {
            updates.openrouter_api_key.is_some()
                || existing
                    .and_then(|e| e.openrouter_api_key.as_ref())
                    .is_some()
        }
    }
}

fn embedding_admission_needed(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    if updates.embedding_provider.is_some()
        || updates.embedding_model.is_some()
        || updates.embedding_dimension.is_some()
    {
        return true;
    }

    let provider = effective_embedding_provider(existing, updates);
    embedding_api_key_changed_for_provider(&provider, updates)
}

fn effective_embedding_provider(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> DeploymentEmbeddingProvider {
    updates
        .embedding_provider
        .clone()
        .or_else(|| existing.map(|e| parse_embedding_provider(&e.embedding_provider)))
        .unwrap_or_else(default_embedding_provider)
}

fn effective_embedding_model_for_provider(
    provider: &DeploymentEmbeddingProvider,
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> String {
    updates
        .embedding_model
        .clone()
        .or_else(|| existing.map(|e| e.embedding_model.clone()))
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default_embedding_model_for_provider(provider))
}

fn effective_embedding_dimension(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> Result<i32, AppError> {
    let dimension = updates
        .embedding_dimension
        .or_else(|| existing.map(|e| e.embedding_dimension))
        .unwrap_or_else(default_embedding_dimension);

    if !is_supported_embedding_dimension(dimension) {
        return Err(AppError::Validation(format!(
            "embedding_dimension must be one of: 1536 or 768 (received {})",
            dimension
        )));
    }

    Ok(dimension)
}

fn embedding_api_key_changed_for_provider(
    provider: &DeploymentEmbeddingProvider,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    match provider {
        DeploymentEmbeddingProvider::Gemini => updates.gemini_api_key.is_some(),
        DeploymentEmbeddingProvider::Openai => updates.openai_api_key.is_some(),
        DeploymentEmbeddingProvider::Openrouter => updates.openrouter_api_key.is_some(),
    }
}

fn effective_embedding_api_key_for_provider(
    provider: &DeploymentEmbeddingProvider,
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
    deps: &impl HasEncryptionProvider,
) -> Result<String, AppError> {
    let (plaintext_update, encrypted_existing) = match provider {
        DeploymentEmbeddingProvider::Gemini => (
            updates.gemini_api_key.as_deref(),
            existing.and_then(|e| e.gemini_api_key.as_deref()),
        ),
        DeploymentEmbeddingProvider::Openai => (
            updates.openai_api_key.as_deref(),
            existing.and_then(|e| e.openai_api_key.as_deref()),
        ),
        DeploymentEmbeddingProvider::Openrouter => (
            updates.openrouter_api_key.as_deref(),
            existing.and_then(|e| e.openrouter_api_key.as_deref()),
        ),
    };

    resolve_api_key(
        plaintext_update,
        encrypted_existing,
        embedding_provider_name(provider),
        deps,
    )
}

fn resolve_api_key(
    plaintext_update: Option<&str>,
    encrypted_existing: Option<&str>,
    provider_name: &str,
    deps: &impl HasEncryptionProvider,
) -> Result<String, AppError> {
    if let Some(plaintext) = plaintext_update {
        return Ok(plaintext.to_string());
    }

    let encrypted = encrypted_existing.ok_or_else(|| {
        AppError::Validation(format!("No API key configured for {provider_name}"))
    })?;

    deps.encryption_provider()
        .decrypt(encrypted)
        .map_err_internal(format!("Failed to decrypt {provider_name} API key"))
}

fn embedding_provider_key(provider: &DeploymentEmbeddingProvider) -> EmbeddingApiProvider {
    match provider {
        DeploymentEmbeddingProvider::Gemini => EmbeddingApiProvider::Gemini,
        DeploymentEmbeddingProvider::Openai => EmbeddingApiProvider::Openai,
        DeploymentEmbeddingProvider::Openrouter => EmbeddingApiProvider::Openrouter,
    }
}

fn llm_provider_name(provider: &DeploymentLlmProvider) -> &'static str {
    match provider {
        DeploymentLlmProvider::Gemini => "Gemini",
        DeploymentLlmProvider::Openai => "OpenAI",
        DeploymentLlmProvider::Openrouter => "OpenRouter",
    }
}

fn embedding_provider_name(provider: &DeploymentEmbeddingProvider) -> &'static str {
    match provider {
        DeploymentEmbeddingProvider::Gemini => "Gemini",
        DeploymentEmbeddingProvider::Openai => "OpenAI",
        DeploymentEmbeddingProvider::Openrouter => "OpenRouter",
    }
}

fn parse_embedding_provider(s: &str) -> DeploymentEmbeddingProvider {
    match s {
        "openai" => DeploymentEmbeddingProvider::Openai,
        "openrouter" => DeploymentEmbeddingProvider::Openrouter,
        _ => DeploymentEmbeddingProvider::Gemini,
    }
}

fn friendly_admission_error(e: AppError) -> String {
    match e {
        AppError::Validation(msg) | AppError::BadRequest(msg) | AppError::Internal(msg) => msg,
        other => other.to_string(),
    }
}
