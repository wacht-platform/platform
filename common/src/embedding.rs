use crate::error::AppError;
use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct EmbedContentRequest {
    model: String,
    content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<i32>,
}

#[derive(Serialize)]
struct BatchEmbedContentsRequest {
    requests: Vec<EmbedContentRequestItem>,
}

#[derive(Serialize)]
struct EmbedContentRequestItem {
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
struct EmbedContentResponse {
    embedding: Embedding,
}

#[derive(Deserialize)]
struct BatchEmbedContentsResponse {
    embeddings: Vec<Embedding>,
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
            model: normalize_rest_model_name(&model),
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
        self.embed_parts(
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
        let api_key = api_key_override.unwrap_or_else(|| self.api_key());
        if api_key.is_empty() {
            return Err(AppError::Internal("GEMINI_API_KEY is not set".to_string()));
        }

        let request = EmbedContentRequest {
            model: self.model().to_string(),
            content: Content {
                parts: build_request_parts(parts),
            },
            output_dimensionality,
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/{}:embedContent",
            self.model()
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
                "Embedding API error: {}",
                error_text
            )));
        }

        let embed_response: EmbedContentResponse = response.json().await.map_err(|e| {
            AppError::Internal(format!("Failed to parse embedding response: {}", e))
        })?;

        Ok(embed_response.embedding.values)
    }

    pub async fn batch_embed_contents(
        &self,
        texts: Vec<String>,
        output_dimensionality: Option<i32>,
        api_key_override: Option<&str>,
    ) -> Result<Vec<Vec<f32>>, AppError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let api_key = api_key_override.unwrap_or_else(|| self.api_key());
        if api_key.is_empty() {
            return Err(AppError::Internal("GEMINI_API_KEY is not set".to_string()));
        }

        const BATCH_SIZE: usize = 100;
        let mut all_embeddings = Vec::new();

        for chunk in texts.chunks(BATCH_SIZE) {
            let requests: Vec<EmbedContentRequestItem> = chunk
                .iter()
                .map(|text| EmbedContentRequestItem {
                    model: self.model().to_string(),
                    content: Content {
                        parts: vec![Part::Text { text: text.clone() }],
                    },
                    output_dimensionality,
                })
                .collect();

            let batch_request = BatchEmbedContentsRequest { requests };
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/{}:batchEmbedContents",
                self.model()
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
                tracing::error!(
                    "Batch embedding API error - Status: {}, URL: {}, Error: {}",
                    status,
                    url,
                    error_text
                );
                return Err(AppError::Internal(format!(
                    "Batch embedding API error ({}): {}",
                    status, error_text
                )));
            }

            let batch_response: BatchEmbedContentsResponse =
                response.json().await.map_err(|e| {
                    AppError::Internal(format!("Failed to parse batch embedding response: {}", e))
                })?;

            all_embeddings.extend(batch_response.embeddings.into_iter().map(|e| e.values));
        }

        Ok(all_embeddings)
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

fn normalize_rest_model_name(model: &str) -> String {
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
