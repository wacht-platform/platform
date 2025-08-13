use serde::{Deserialize, Serialize};
use common::error::AppError;
use tracing::error;

const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiResponse {
    pub candidates: Vec<Candidate>,
    #[serde(rename = "usageMetadata")]
    pub usage_metadata: Option<UsageMetadata>,
    #[serde(rename = "modelVersion")]
    pub model_version: Option<String>,
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
            client: reqwest::Client::new(),
        }
    }

    pub async fn generate_structured_content<T>(&self, request_body: String) -> Result<T, AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);

        let mut last_error = None;
        for attempt in 1..=3 {
            if attempt > 1 {
                tracing::warn!("Retrying Gemini API request (attempt {})", attempt);
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }

            let response = self
                .client
                .post(&url)
                .header("x-goog-api-key", &self.api_key)
                .header("Content-Type", "application/json")
                .body(request_body.clone())
                .send()
                .await;

            match response {
                Ok(resp) => {
                    let body = resp.bytes().await;
                    match body {
                        Ok(bytes) => match serde_json::from_slice::<GeminiResponse>(&bytes) {
                            Ok(gemini_response) => {
                                let mut accumulated_text = String::new();
                                for part in &gemini_response.candidates[0].content.parts {
                                    accumulated_text.push_str(&part.text);
                                }

                                if accumulated_text.is_empty() {
                                    last_error =
                                        Some("No response content from Gemini API".to_string());
                                    continue;
                                }

                                match serde_json::from_str::<T>(&accumulated_text) {
                                    Ok(parsed_response) => {
                                        return Ok(parsed_response);
                                    }
                                    Err(e) => {
                                        error!("Failed to parse structured response: {}", e);
                                        error!("Raw response: {}", accumulated_text);
                                        last_error =
                                            Some(format!("Failed to parse response: {e}"));
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to parse Gemini response: {}", e);
                                error!("Raw body: {:?}", String::from_utf8_lossy(&bytes));
                                last_error = Some(format!("Invalid API response format: {e}"));
                            }
                        },
                        Err(e) => {
                            last_error = Some(format!("Failed to read response body: {e}"));
                        }
                    }
                }
                Err(e) => {
                    last_error = Some(format!("Request failed: {e}"));
                }
            }
        }

        Err(AppError::Internal(format!(
            "Failed after 3 attempts: {}",
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        )))
    }
}
