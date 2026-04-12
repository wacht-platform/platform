use common::{
    EmbeddingPart, HasDbRouter, HasEmbeddingProvider, HasEncryptionProvider,
    db_router::ReadConsistency, error::AppError,
};

const KNOWLEDGE_EMBEDDING_DIMENSION: i32 = 1536;
const RETRIEVAL_DOCUMENT_TASK_TYPE: &str = "RETRIEVAL_DOCUMENT";
const RETRIEVAL_QUERY_TASK_TYPE: &str = "RETRIEVAL_QUERY";

fn format_embedding_input(
    model: &str,
    text: &str,
    title: Option<&str>,
    task_type: Option<&str>,
) -> String {
    if !model.contains("gemini-embedding-2-preview") {
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

pub async fn resolve_deployment_gemini_api_key<D>(
    deps: &D,
    deployment_id: i64,
) -> Result<Option<String>, AppError>
where
    D: HasDbRouter + HasEncryptionProvider + ?Sized,
{
    let reader = deps.db_router().reader(ReadConsistency::Strong);
    let settings = queries::GetDeploymentAiSettingsQuery::new(deployment_id)
        .execute_with_db(reader)
        .await?;

    match settings.and_then(|s| s.gemini_api_key) {
        Some(encrypted) if !encrypted.is_empty() => {
            Ok(Some(deps.encryption_provider().decrypt(&encrypted)?))
        }
        _ => Ok(None),
    }
}

pub fn format_retrieval_query_input(model: &str, text: &str) -> String {
    format_embedding_input(model, text, None, Some(RETRIEVAL_QUERY_TASK_TYPE))
}

pub fn format_retrieval_document_input(model: &str, text: &str, title: Option<&str>) -> String {
    format_embedding_input(model, text, title, Some(RETRIEVAL_DOCUMENT_TASK_TYPE))
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
        let api_key_override = if let Some(deployment_id) = self.deployment_id {
            resolve_deployment_gemini_api_key(deps, deployment_id).await?
        } else {
            None
        };

        let formatted_text = format_embedding_input(
            deps.embedding_provider().model(),
            &self.text,
            self.title.as_deref(),
            Some(if self.is_retrieval_query {
                RETRIEVAL_QUERY_TASK_TYPE
            } else {
                RETRIEVAL_DOCUMENT_TASK_TYPE
            }),
        );

        deps.embedding_provider()
            .embed_content(
                formatted_text,
                Some(KNOWLEDGE_EMBEDDING_DIMENSION),
                api_key_override.as_deref(),
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
        let api_key_override = if let Some(deployment_id) = self.deployment_id {
            resolve_deployment_gemini_api_key(deps, deployment_id).await?
        } else {
            None
        };

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
                    deps.embedding_provider().model(),
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
            .batch_embed_contents(
                formatted_texts,
                Some(KNOWLEDGE_EMBEDDING_DIMENSION),
                api_key_override.as_deref(),
            )
            .await
    }
}
