use crate::error::AppError;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct EmbedContentRequest {
    model: String,
    content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_type: Option<String>,
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
    task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<i32>,
}

#[derive(Serialize)]
struct Content {
    parts: Vec<Part>,
}

#[derive(Serialize)]
struct Part {
    text: String,
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
            model,
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
        task_type: Option<String>,
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
                parts: vec![Part { text }],
            },
            task_type,
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
        task_type: Option<String>,
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
                        parts: vec![Part { text: text.clone() }],
                    },
                    task_type: task_type.clone(),
                    output_dimensionality,
                })
                .collect();

            let batch_request = BatchEmbedContentsRequest { requests };
            let url = format!(
                "https://generativelanguage.googleapis.com/v1/{}:batchEmbedContents",
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
