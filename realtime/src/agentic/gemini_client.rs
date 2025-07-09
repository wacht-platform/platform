use std::time::Duration;

use eventsource_client::{self as es, Client, ReconnectOptions};
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use shared::error::AppError;
use tracing::{debug, error};

const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    model: String,
}

// Request structures are now defined in templates as JSON

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiStreamResponse {
    pub candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(rename = "modelVersion")]
    pub model_version: Option<String>,
    #[serde(rename = "responseId")]
    pub response_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub content: CandidateContent,
    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>,
    pub index: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CandidateContent {
    pub parts: Vec<CandidatePart>,
    pub role: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CandidatePart {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    pub prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    pub candidates_token_count: u32,
    #[serde(rename = "totalTokenCount")]
    pub total_token_count: u32,
}

impl GeminiClient {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "gemini-2.0-flash-exp".to_string()),
        }
    }

    pub async fn generate_structured_content<T>(
        &self,
        request_body: String,
    ) -> Result<(String, T), AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let url = format!(
            "{}/{}:streamGenerateContent?alt=sse",
            GEMINI_API_BASE_URL, self.model
        );

        let body = request_body;

        let client = es::ClientBuilder::for_url(&url)
            .map_err(|e| AppError::Internal(format!("Failed to build client: {}", e)))?
            .header("x-goog-api-key", &self.api_key)
            .map_err(|e| AppError::Internal(format!("Failed to set API key header: {}", e)))?
            .header("Content-Type", "application/json")
            .map_err(|e| AppError::Internal(format!("Failed to set Content-Type header: {}", e)))?
            .method("POST".to_string())
            .reconnect(ReconnectOptions::reconnect(false).build())
            .read_timeout(Duration::from_secs(12000000))
            .body(body)
            .build();

        let mut accumulated_text = String::new();

        let mut stream = client
            .stream()
            .map_ok(|event| match event {
                es::SSE::Event(ev) => {
                    if ev.data.trim().is_empty() {
                        return;
                    }

                    match serde_json::from_str::<GeminiStreamResponse>(&ev.data) {
                        Ok(response) => {
                            for candidate in &response.candidates {
                                for part in &candidate.content.parts {
                                    accumulated_text.push_str(&part.text);
                                }
                            }

                            if response
                                .candidates
                                .iter()
                                .any(|c| c.finish_reason.is_some())
                            {}
                        }
                        Err(e) => {
                            error!("Failed to parse SSE data: {}, data: {}", e, ev.data);
                        }
                    }
                }
                es::SSE::Comment(_) => {}
                es::SSE::Connected(_) => {
                    debug!("Connected to Gemini API");
                }
            })
            .map_err(|err| {
                tracing::error!("Gemini API streaming error: {:?}", err);
                eprintln!("error streaming events: {:?}", err)
            });

        while let Ok(Some(_)) = stream.try_next().await {}

        if accumulated_text.is_empty() {
            return Err(AppError::Internal(
                "No response received from Gemini API".to_string(),
            ));
        }

        let parsed_response = serde_json::from_str::<T>(&accumulated_text).map_err(|e| {
            tracing::error!("Failed to parse response: {}, Raw: {}", e, accumulated_text);
            AppError::Internal(format!("Failed to parse structured response: {}", e))
        })?;

        Ok((accumulated_text, parsed_response))
    }
}
