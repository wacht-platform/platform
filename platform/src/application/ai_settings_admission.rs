use common::{EmbeddingApiProvider, HasEmbeddingProvider, HasEncryptionProvider, error::AppError};
use models::{
    DeploymentAiSettings, DeploymentEmbeddingProvider, DeploymentLlmProvider,
    UpdateDeploymentAiSettingsRequest, default_embedding_dimension,
    default_embedding_model_for_provider, default_embedding_provider,
    is_supported_embedding_dimension,
};

pub(super) async fn run_model_admission_if_needed(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
    deps: &(impl HasEncryptionProvider + HasEmbeddingProvider),
) -> Result<(), AppError> {
    if strong_admission_needed(existing, updates) {
        let provider = effective_strong_provider(existing, updates);
        let model = effective_strong_model_for_provider(&provider, existing, updates);
        let api_key = effective_api_key_for_provider(&provider, existing, updates, deps)?;
        let require_parameters = effective_openrouter_require_parameters(existing, updates);

        agent_engine::admission::admit_strong_model(
            llm_provider_key(&provider),
            &model,
            Some(api_key.as_str()),
            require_parameters,
        )
        .await
        .map_err(|e| {
            AppError::Validation(format!(
                "Strong model admission failed — {} cannot be saved. The agent's decision loop requires a model that emits correct native tool calls. Details: {}",
                llm_provider_name(&provider),
                friendly_admission_error(e),
            ))
        })?;
    }

    if weak_admission_needed(existing, updates) {
        let provider = effective_weak_provider(existing, updates);
        let model = effective_weak_model_for_provider(&provider, existing, updates);
        let api_key = effective_api_key_for_provider(&provider, existing, updates, deps)?;
        let require_parameters = effective_openrouter_require_parameters(existing, updates);

        agent_engine::admission::admit_weak_model(
            llm_provider_key(&provider),
            &model,
            Some(api_key.as_str()),
            require_parameters,
        )
        .await
        .map_err(|e| {
            AppError::Validation(format!(
                "Weak model admission failed — {} cannot be saved. The weak model is used for conversation compaction and tool-catalog search; it must return valid structured output. Details: {}",
                llm_provider_name(&provider),
                friendly_admission_error(e),
            ))
        })?;
    }

    Ok(())
}

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

pub(super) fn validate_openrouter_strong_model(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> Result<(), AppError> {
    let effective_strong_provider = updates
        .strong_llm_provider
        .as_ref()
        .cloned()
        .or_else(|| existing.map(|e| parse_llm_provider(&e.strong_llm_provider)));

    if !matches!(
        effective_strong_provider.as_ref(),
        Some(DeploymentLlmProvider::Openrouter)
    ) {
        return Ok(());
    }

    let effective_require_parameters = effective_openrouter_require_parameters(existing, updates);

    if !effective_require_parameters {
        return Err(AppError::Validation(
            "OpenRouter is selected as the strong model provider but 'require parameters' is disabled. The strong model drives tool/function calling — enable 'require parameters' so OpenRouter only routes to endpoints that support the `tools` field."
                .to_string(),
        ));
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

fn strong_admission_needed(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    if updates.strong_llm_provider.is_some() || updates.strong_model.is_some() {
        return true;
    }

    let provider = effective_strong_provider(existing, updates);
    llm_api_key_changed_for_provider(&provider, updates)
}

fn weak_admission_needed(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    if updates.weak_llm_provider.is_some() || updates.weak_model.is_some() {
        return true;
    }

    let provider = effective_weak_provider(existing, updates);
    llm_api_key_changed_for_provider(&provider, updates)
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

fn effective_strong_provider(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> DeploymentLlmProvider {
    updates
        .strong_llm_provider
        .clone()
        .or_else(|| existing.map(|e| parse_llm_provider(&e.strong_llm_provider)))
        .unwrap_or(DeploymentLlmProvider::Gemini)
}

fn effective_weak_provider(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> DeploymentLlmProvider {
    updates
        .weak_llm_provider
        .clone()
        .or_else(|| existing.map(|e| parse_llm_provider(&e.weak_llm_provider)))
        .unwrap_or(DeploymentLlmProvider::Gemini)
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

fn effective_strong_model_for_provider(
    provider: &DeploymentLlmProvider,
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> String {
    updates
        .strong_model
        .clone()
        .or_else(|| existing.and_then(|e| e.strong_model.clone()))
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default_llm_model_for_provider(provider, true))
}

fn effective_weak_model_for_provider(
    provider: &DeploymentLlmProvider,
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> String {
    updates
        .weak_model
        .clone()
        .or_else(|| existing.and_then(|e| e.weak_model.clone()))
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default_llm_model_for_provider(provider, false))
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

fn default_llm_model_for_provider(provider: &DeploymentLlmProvider, strong: bool) -> String {
    match (provider, strong) {
        (DeploymentLlmProvider::Gemini, true) => "gemini-3.1-pro-preview".to_string(),
        (DeploymentLlmProvider::Gemini, false) => "gemini-3-flash-preview".to_string(),
        (DeploymentLlmProvider::Openai, true) => "gpt-5.1".to_string(),
        (DeploymentLlmProvider::Openai, false) => "gpt-5-mini".to_string(),
        (DeploymentLlmProvider::Openrouter, _) => {
            "nvidia/nemotron-3-super-120b-a12b:free".to_string()
        }
    }
}

fn effective_openrouter_require_parameters(
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    updates
        .openrouter_require_parameters
        .or_else(|| existing.map(|e| e.openrouter_require_parameters))
        .unwrap_or(true)
}

fn llm_api_key_changed_for_provider(
    provider: &DeploymentLlmProvider,
    updates: &UpdateDeploymentAiSettingsRequest,
) -> bool {
    match provider {
        DeploymentLlmProvider::Gemini => updates.gemini_api_key.is_some(),
        DeploymentLlmProvider::Openai => updates.openai_api_key.is_some(),
        DeploymentLlmProvider::Openrouter => updates.openrouter_api_key.is_some(),
    }
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

fn effective_api_key_for_provider(
    provider: &DeploymentLlmProvider,
    existing: Option<&DeploymentAiSettings>,
    updates: &UpdateDeploymentAiSettingsRequest,
    deps: &impl HasEncryptionProvider,
) -> Result<String, AppError> {
    let (plaintext_update, encrypted_existing) = match provider {
        DeploymentLlmProvider::Gemini => (
            updates.gemini_api_key.as_deref(),
            existing.and_then(|e| e.gemini_api_key.as_deref()),
        ),
        DeploymentLlmProvider::Openai => (
            updates.openai_api_key.as_deref(),
            existing.and_then(|e| e.openai_api_key.as_deref()),
        ),
        DeploymentLlmProvider::Openrouter => (
            updates.openrouter_api_key.as_deref(),
            existing.and_then(|e| e.openrouter_api_key.as_deref()),
        ),
    };

    resolve_api_key(
        plaintext_update,
        encrypted_existing,
        llm_provider_name(provider),
        deps,
    )
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
        .map_err(|e| AppError::Internal(format!("Failed to decrypt {provider_name} API key: {e}")))
}

fn llm_provider_key(provider: &DeploymentLlmProvider) -> &'static str {
    match provider {
        DeploymentLlmProvider::Gemini => "gemini",
        DeploymentLlmProvider::Openai => "openai",
        DeploymentLlmProvider::Openrouter => "openrouter",
    }
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

fn parse_llm_provider(s: &str) -> DeploymentLlmProvider {
    match s {
        "openai" => DeploymentLlmProvider::Openai,
        "openrouter" => DeploymentLlmProvider::Openrouter,
        _ => DeploymentLlmProvider::Gemini,
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
