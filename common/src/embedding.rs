use crate::error::AppError;
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddingApiProvider {
    Gemini,
    Openai,
    Openrouter,
}

#[derive(Serialize)]
struct GeminiEmbedContentRequest {
    model: String,
    content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<i32>,
}

#[derive(Serialize)]
struct GeminiBatchEmbedContentsRequest {
    requests: Vec<GeminiEmbedContentRequestItem>,
}

#[derive(Serialize)]
struct GeminiEmbedContentRequestItem {
    model: String,
    content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<i32>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Clone)]
pub enum EmbeddingPart {
    Text(String),
    InlineData { mime_type: String, data: Vec<u8> },
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part {
    Text {
        text: String,
    },
    InlineData {
        #[serde(rename = "inline_data")]
        inline_data: InlineData,
    },
}

#[derive(Serialize)]
struct InlineData {
    mime_type: String,
    data: String,
}

#[derive(Deserialize)]
struct GeminiEmbedContentResponse {
    embedding: Embedding,
}

#[derive(Deserialize)]
struct GeminiBatchEmbedContentsResponse {
    embeddings: Vec<Embedding>,
}

#[derive(Serialize)]
struct OpenAiEmbeddingRequestSingle {
    model: String,
    input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<i32>,
}

#[derive(Serialize)]
struct OpenAiEmbeddingRequestBatch {
    model: String,
    input: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<i32>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbeddingItem>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingItem {
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct Embedding {
    values: Vec<f32>,
}

#[derive(Clone)]
pub struct EmbeddingProvider {
    http_client: reqwest::Client,
    api_key: String,
    model: String,
}

impl EmbeddingProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            api_key,
            model: normalize_gemini_model_name(&model),
        }
    }

    pub fn http_client(&self) -> &reqwest::Client {
        &self.http_client
    }

    pub fn api_key(&self) -> &str {
        &self.api_key
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn embed_content(
        &self,
        text: String,
        output_dimensionality: Option<i32>,
        api_key_override: Option<&str>,
    ) -> Result<Vec<f32>, AppError> {
        self.embed_content_with(
            EmbeddingApiProvider::Gemini,
            self.model(),
            text,
            output_dimensionality,
            api_key_override,
        )
        .await
    }

    pub async fn embed_content_with(
        &self,
        provider: EmbeddingApiProvider,
        model: &str,
        text: String,
        output_dimensionality: Option<i32>,
        api_key_override: Option<&str>,
    ) -> Result<Vec<f32>, AppError> {
        self.embed_parts_with(
            provider,
            model,
            vec![EmbeddingPart::Text(text)],
            output_dimensionality,
            api_key_override,
        )
        .await
    }

    pub async fn embed_parts(
        &self,
        parts: Vec<EmbeddingPart>,
        output_dimensionality: Option<i32>,
        api_key_override: Option<&str>,
    ) -> Result<Vec<f32>, AppError> {
        self.embed_parts_with(
            EmbeddingApiProvider::Gemini,
            self.model(),
            parts,
            output_dimensionality,
            api_key_override,
        )
        .await
    }

    pub async fn embed_parts_with(
        &self,
        provider: EmbeddingApiProvider,
        model: &str,
        parts: Vec<EmbeddingPart>,
        output_dimensionality: Option<i32>,
        api_key_override: Option<&str>,
    ) -> Result<Vec<f32>, AppError> {
        let api_key = api_key_override.unwrap_or_else(|| self.api_key());
        if api_key.is_empty() {
            return Err(AppError::Validation(
                "Embedding API key is not configured".to_string(),
            ));
        }

        match provider {
            EmbeddingApiProvider::Gemini => {
                self.gemini_embed_parts(model, parts, output_dimensionality, api_key)
                    .await
            }
            EmbeddingApiProvider::Openai | EmbeddingApiProvider::Openrouter => {
                if parts
                    .iter()
                    .any(|part| matches!(part, EmbeddingPart::InlineData { .. }))
                {
                    return Err(AppError::Validation(
                        "Multimodal embedding parts are only supported for Gemini".to_string(),
                    ));
                }
                let text = parts
                    .into_iter()
                    .filter_map(|part| match part {
                        EmbeddingPart::Text(text) => Some(text),
                        EmbeddingPart::InlineData { .. } => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n");
                self.openai_compatible_embed_single(
                    provider,
                    model,
                    text,
                    output_dimensionality,
                    api_key,
                )
                .await
            }
        }
    }

    pub async fn batch_embed_contents(
        &self,
        texts: Vec<String>,
        output_dimensionality: Option<i32>,
        api_key_override: Option<&str>,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        self.batch_embed_contents_with(
            EmbeddingApiProvider::Gemini,
            self.model(),
            texts,
            output_dimensionality,
            api_key_override,
        )
        .await
    }

    pub async fn batch_embed_contents_with(
        &self,
        provider: EmbeddingApiProvider,
        model: &str,
        texts: Vec<String>,
        output_dimensionality: Option<i32>,
        api_key_override: Option<&str>,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let api_key = api_key_override.unwrap_or_else(|| self.api_key());
        if api_key.is_empty() {
            return Err(AppError::Validation(
                "Embedding API key is not configured".to_string(),
            ));
        }

        match provider {
            EmbeddingApiProvider::Gemini => {
                self.gemini_batch_embed_contents(model, texts, output_dimensionality, api_key)
                    .await
            }
            EmbeddingApiProvider::Openai | EmbeddingApiProvider::Openrouter => {
                self.openai_compatible_embed_batch(
                    provider,
                    model,
                    texts,
                    output_dimensionality,
                    api_key,
                )
                .await
            }
        }
    }

    async fn gemini_embed_parts(
        &self,
        model: &str,
        parts: Vec<EmbeddingPart>,
        output_dimensionality: Option<i32>,
        api_key: &str,
    ) -> Result<Vec<f32>, AppError> {
        let model = normalize_gemini_model_name(model);
        let request = GeminiEmbedContentRequest {
            model: model.clone(),
            content: Content {
                parts: build_request_parts(parts),
            },
            output_dimensionality,
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{}:embedContent",
            model
        );

        let response = self
            .http_client()
            .post(&url)
            .header("x-goog-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to send embedding request: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Gemini embedding API error: {}",
                error_text
            )));
        }

        let embed_response: GeminiEmbedContentResponse = response.json().await.map_err(|e| {
            AppError::Internal(format!("Failed to parse embedding response: {}", e))
        })?;

        Ok(embed_response.embedding.values)
    }

    async fn gemini_batch_embed_contents(
        &self,
        model: &str,
        texts: Vec<String>,
        output_dimensionality: Option<i32>,
        api_key: &str,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        const BATCH_SIZE: usize = 100;
        let model = normalize_gemini_model_name(model);
        let mut all_embeddings = Vec::new();

        for chunk in texts.chunks(BATCH_SIZE) {
            let requests: Vec<GeminiEmbedContentRequestItem> = chunk
                .iter()
                .map(|text| GeminiEmbedContentRequestItem {
                    model: model.clone(),
                    content: Content {
                        parts: vec![Part::Text { text: text.clone() }],
                    },
                    output_dimensionality,
                })
                .collect();

            let batch_request = GeminiBatchEmbedContentsRequest { requests };
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/{}:batchEmbedContents",
                model
            );

            let response = self
                .http_client()
                .post(&url)
                .header("x-goog-api-key", api_key)
                .header("Content-Type", "application/json")
                .json(&batch_request)
                .send()
                .await
                .map_err(|e| {
                    AppError::Internal(format!("Failed to send batch embedding request: {}", e))
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response.text().await.unwrap_or_default();
                return Err(AppError::Internal(format!(
                    "Gemini batch embedding API error ({}): {}",
                    status, error_text
                )));
            }

            let batch_response: GeminiBatchEmbedContentsResponse =
                response.json().await.map_err(|e| {
                    AppError::Internal(format!("Failed to parse batch embedding response: {}", e))
                })?;

            all_embeddings.extend(batch_response.embeddings.into_iter().map(|e| e.values));
        }

        Ok(all_embeddings)
    }

    async fn openai_compatible_embed_single(
        &self,
        provider: EmbeddingApiProvider,
        model: &str,
        text: String,
        output_dimensionality: Option<i32>,
        api_key: &str,
    ) -> Result<Vec<f32>, AppError> {
        let request = OpenAiEmbeddingRequestSingle {
            model: model.trim().to_string(),
            input: text,
            dimensions: output_dimensionality,
        };

        let response = self
            .http_client()
            .post(openai_compatible_embedding_url(provider))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to send embedding request: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "{} embedding API error ({}): {}",
                provider_name(provider),
                status,
                error_text
            )));
        }

        let embed_response: OpenAiEmbeddingResponse = response.json().await.map_err(|e| {
            AppError::Internal(format!("Failed to parse embedding response: {}", e))
        })?;

        embed_response
            .data
            .into_iter()
            .next()
            .map(|item| item.embedding)
            .ok_or_else(|| {
                AppError::Internal(format!(
                    "{} embedding API returned no data",
                    provider_name(provider)
                ))
            })
    }

    async fn openai_compatible_embed_batch(
        &self,
        provider: EmbeddingApiProvider,
        model: &str,
        texts: Vec<String>,
        output_dimensionality: Option<i32>,
        api_key: &str,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        let request = OpenAiEmbeddingRequestBatch {
            model: model.trim().to_string(),
            input: texts,
            dimensions: output_dimensionality,
        };

        let response = self
            .http_client()
            .post(openai_compatible_embedding_url(provider))
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to send embedding request: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "{} batch embedding API error ({}): {}",
                provider_name(provider),
                status,
                error_text
            )));
        }

        let embed_response: OpenAiEmbeddingResponse = response.json().await.map_err(|e| {
            AppError::Internal(format!("Failed to parse batch embedding response: {}", e))
        })?;

        Ok(embed_response
            .data
            .into_iter()
            .map(|item| item.embedding)
            .collect())
    }
}

fn build_request_parts(parts: Vec<EmbeddingPart>) -> Vec<Part> {
    parts
        .into_iter()
        .map(|part| match part {
            EmbeddingPart::Text(text) => Part::Text { text },
            EmbeddingPart::InlineData { mime_type, data } => Part::InlineData {
                inline_data: InlineData {
                    mime_type,
                    data: base64::engine::general_purpose::STANDARD.encode(data),
                },
            },
        })
        .collect()
}

fn normalize_gemini_model_name(model: &str) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        return "models/gemini-embedding-2-preview".to_string();
    }

    if trimmed.starts_with("models/") {
        trimmed.to_string()
    } else {
        format!("models/{}", trimmed)
    }
}

fn provider_name(provider: EmbeddingApiProvider) -> &'static str {
    match provider {
        EmbeddingApiProvider::Gemini => "Gemini",
        EmbeddingApiProvider::Openai => "OpenAI",
        EmbeddingApiProvider::Openrouter => "OpenRouter",
    }
}

fn openai_compatible_embedding_url(provider: EmbeddingApiProvider) -> &'static str {
    match provider {
        // Gemini uses its own native endpoint and must not reach this function.
        EmbeddingApiProvider::Gemini => {
            unreachable!("openai_compatible_embedding_url called with Gemini provider")
        }
        EmbeddingApiProvider::Openai => "https://api.openai.com/v1/embeddings",
        EmbeddingApiProvider::Openrouter => "https://openrouter.ai/api/v1/embeddings",
    }
}
