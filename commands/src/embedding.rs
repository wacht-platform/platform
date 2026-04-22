use common::{
    EmbeddingApiProvider, EmbeddingPart, HasDbRouter, HasEmbeddingProvider, HasEncryptionProvider,
    db_router::ReadConsistency, error::AppError,
};
use models::{
    DeploymentEmbeddingProvider, default_embedding_dimension, default_embedding_model_for_provider,
    default_embedding_provider, is_supported_embedding_dimension,
};

const RETRIEVAL_DOCUMENT_TASK_TYPE: &str = "RETRIEVAL_DOCUMENT";
const RETRIEVAL_QUERY_TASK_TYPE: &str = "RETRIEVAL_QUERY";

#[derive(Clone)]
pub struct ResolvedEmbeddingSettings {
    pub provider: DeploymentEmbeddingProvider,
    pub model: String,
    pub api_key: String,
    pub dimension: i32,
}

fn format_embedding_input(
    provider: &DeploymentEmbeddingProvider,
    model: &str,
    text: &str,
    title: Option<&str>,
    task_type: Option<&str>,
) -> String {
    if !matches!(provider, DeploymentEmbeddingProvider::Gemini)
        || !model.contains("gemini-embedding-2-preview")
    {
        return text.to_string();
    }

    match task_type {
        Some(RETRIEVAL_QUERY_TASK_TYPE) => format!("task: search result | query: {}", text),
        _ => format!(
            "title: {} | text: {}",
            title
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("none"),
            text
        ),
    }
}

fn map_embedding_provider(provider: &DeploymentEmbeddingProvider) -> EmbeddingApiProvider {
    match provider {
        DeploymentEmbeddingProvider::Gemini => EmbeddingApiProvider::Gemini,
        DeploymentEmbeddingProvider::Openai => EmbeddingApiProvider::Openai,
        DeploymentEmbeddingProvider::Openrouter => EmbeddingApiProvider::Openrouter,
    }
}

fn provider_name(provider: &DeploymentEmbeddingProvider) -> &'static str {
    match provider {
        DeploymentEmbeddingProvider::Gemini => "Gemini",
        DeploymentEmbeddingProvider::Openai => "OpenAI",
        DeploymentEmbeddingProvider::Openrouter => "OpenRouter",
    }
}

pub async fn resolve_deployment_embedding_settings<D>(
    deps: &D,
    deployment_id: i64,
) -> Result<ResolvedEmbeddingSettings, AppError>
where
    D: HasDbRouter + HasEncryptionProvider + ?Sized,
{
    let reader = deps.db_router().reader(ReadConsistency::Strong);
    let settings = queries::GetDeploymentAiSettingsQuery::new(deployment_id)
        .execute_with_db(reader)
        .await?;

    let provider = settings
        .as_ref()
        .map(|s| match s.embedding_provider.as_str() {
            "openai" => DeploymentEmbeddingProvider::Openai,
            "openrouter" => DeploymentEmbeddingProvider::Openrouter,
            _ => DeploymentEmbeddingProvider::Gemini,
        })
        .unwrap_or_else(default_embedding_provider);

    let model = settings
        .as_ref()
        .map(|s| s.embedding_model.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default_embedding_model_for_provider(&provider));

    let dimension = settings
        .as_ref()
        .map(|s| s.embedding_dimension)
        .unwrap_or_else(default_embedding_dimension);
    if !is_supported_embedding_dimension(dimension) {
        return Err(AppError::Validation(format!(
            "Unsupported deployment embedding_dimension {}. Supported values: 1536, 768.",
            dimension
        )));
    }

    let encrypted_key = settings.as_ref().and_then(|s| match provider {
        DeploymentEmbeddingProvider::Gemini => s.gemini_api_key.clone(),
        DeploymentEmbeddingProvider::Openai => s.openai_api_key.clone(),
        DeploymentEmbeddingProvider::Openrouter => s.openrouter_api_key.clone(),
    });

    let encrypted_key = encrypted_key.filter(|value| !value.trim().is_empty());
    let encrypted_key = encrypted_key.ok_or_else(|| {
        AppError::Validation(format!(
            "{} API key is required for embeddings. Configure embedding_provider/model and key in deployment AI settings.",
            provider_name(&provider)
        ))
    })?;

    let api_key = deps
        .encryption_provider()
        .decrypt(&encrypted_key)
        .map_err(|e| {
            AppError::Internal(format!(
                "Failed to decrypt deployment {} API key for embeddings: {e}",
                provider_name(&provider)
            ))
        })?;

    if api_key.trim().is_empty() {
        return Err(AppError::Validation(format!(
            "{} API key is required for embeddings",
            provider_name(&provider)
        )));
    }

    Ok(ResolvedEmbeddingSettings {
        provider,
        model,
        api_key,
        dimension,
    })
}

pub async fn resolve_deployment_embedding_dimension<D>(
    deps: &D,
    deployment_id: i64,
) -> Result<i32, AppError>
where
    D: HasDbRouter + HasEncryptionProvider + ?Sized,
{
    Ok(resolve_deployment_embedding_settings(deps, deployment_id)
        .await?
        .dimension)
}

pub fn format_retrieval_query_input(model: &str, text: &str) -> String {
    format_embedding_input(
        &DeploymentEmbeddingProvider::Gemini,
        model,
        text,
        None,
        Some(RETRIEVAL_QUERY_TASK_TYPE),
    )
}

pub fn format_retrieval_document_input(model: &str, text: &str, title: Option<&str>) -> String {
    format_embedding_input(
        &DeploymentEmbeddingProvider::Gemini,
        model,
        text,
        title,
        Some(RETRIEVAL_DOCUMENT_TASK_TYPE),
    )
}

pub fn build_multimodal_retrieval_document_parts(
    model: &str,
    text: &str,
    title: Option<&str>,
    mime_type: &str,
    data: Vec<u8>,
) -> Vec<EmbeddingPart> {
    vec![
        EmbeddingPart::Text(format_retrieval_document_input(model, text, title)),
        EmbeddingPart::InlineData {
            mime_type: mime_type.to_string(),
            data,
        },
    ]
}

#[derive(Clone)]
pub struct GenerateEmbeddingCommand {
    pub text: String,
    pub title: Option<String>,
    is_retrieval_query: bool,
    pub deployment_id: Option<i64>,
}

impl GenerateEmbeddingCommand {
    pub fn new(text: String) -> Self {
        Self {
            text,
            title: None,
            is_retrieval_query: false,
            deployment_id: None,
        }
    }

    pub fn with_title(mut self, title: Option<String>) -> Self {
        self.title = title;
        self
    }

    pub fn for_retrieval_query(mut self) -> Self {
        self.is_retrieval_query = true;
        self
    }

    pub fn for_retrieval_document(mut self) -> Self {
        self.is_retrieval_query = false;
        self
    }

    pub fn for_deployment(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<f32>, AppError>
    where
        D: HasEmbeddingProvider + HasDbRouter + HasEncryptionProvider + ?Sized,
    {
        let deployment_id = self.deployment_id.ok_or_else(|| {
            AppError::Validation("deployment_id is required for embedding generation".to_string())
        })?;
        let settings = resolve_deployment_embedding_settings(deps, deployment_id).await?;

        let formatted_text = format_embedding_input(
            &settings.provider,
            &settings.model,
            &self.text,
            self.title.as_deref(),
            Some(if self.is_retrieval_query {
                RETRIEVAL_QUERY_TASK_TYPE
            } else {
                RETRIEVAL_DOCUMENT_TASK_TYPE
            }),
        );

        deps.embedding_provider()
            .embed_content_with(
                map_embedding_provider(&settings.provider),
                &settings.model,
                formatted_text,
                Some(settings.dimension),
                Some(settings.api_key.as_str()),
            )
            .await
    }
}

#[derive(Clone)]
pub struct GenerateEmbeddingsCommand {
    pub texts: Vec<String>,
    pub titles: Option<Vec<Option<String>>>,
    is_retrieval_query: bool,
    pub deployment_id: Option<i64>,
}

impl GenerateEmbeddingsCommand {
    pub fn new(texts: Vec<String>) -> Self {
        Self {
            texts,
            titles: None,
            is_retrieval_query: false,
            deployment_id: None,
        }
    }

    pub fn with_titles(mut self, titles: Vec<Option<String>>) -> Self {
        self.titles = Some(titles);
        self
    }

    pub fn for_retrieval_query(mut self) -> Self {
        self.is_retrieval_query = true;
        self
    }

    pub fn for_retrieval_document(mut self) -> Self {
        self.is_retrieval_query = false;
        self
    }

    pub fn for_deployment(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<Vec<f32>>, AppError>
    where
        D: HasEmbeddingProvider + HasDbRouter + HasEncryptionProvider + ?Sized,
    {
        let deployment_id = self.deployment_id.ok_or_else(|| {
            AppError::Validation("deployment_id is required for embedding generation".to_string())
        })?;
        let settings = resolve_deployment_embedding_settings(deps, deployment_id).await?;

        let formatted_texts = self
            .texts
            .into_iter()
            .enumerate()
            .map(|(index, text)| {
                let title = self
                    .titles
                    .as_ref()
                    .and_then(|titles| titles.get(index))
                    .and_then(|value| value.as_deref());
                format_embedding_input(
                    &settings.provider,
                    &settings.model,
                    &text,
                    title,
                    Some(if self.is_retrieval_query {
                        RETRIEVAL_QUERY_TASK_TYPE
                    } else {
                        RETRIEVAL_DOCUMENT_TASK_TYPE
                    }),
                )
            })
            .collect();

        deps.embedding_provider()
            .batch_embed_contents_with(
                map_embedding_provider(&settings.provider),
                &settings.model,
                formatted_texts,
                Some(settings.dimension),
                Some(settings.api_key.as_str()),
            )
            .await
    }
}
