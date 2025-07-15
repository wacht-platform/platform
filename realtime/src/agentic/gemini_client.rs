use serde::{Deserialize, Serialize};
use shared::error::AppError;
use tracing::error;

const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    model: String,
}

// Request structures are now defined in templates as JSON

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
        }
    }

    pub async fn generate_structured_content<T>(
        &self,
        request_body: String,
    ) -> Result<(String, T), AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);

        let mut response = match ureq::post(&url)
            .header("x-goog-api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .send(request_body.as_bytes())
        {
            Ok(resp) => resp,
            Err(e) => {
                error!("Failed to make Gemini API request: {}", e);
                return Err(AppError::Internal(format!(
                    "Gemini API request failed: {}",
                    e
                )));
            }
        };

        let gemini_response: GeminiResponse = response.body_mut().read_json().map_err(|e| {
            error!("Failed to read Gemini API response: {}", e);
            AppError::Internal(format!("Failed to read response: {}", e))
        })?;

        // Extract text from all candidates
        let mut accumulated_text = String::new();
        for candidate in &gemini_response.candidates {
            for part in &candidate.content.parts {
                accumulated_text.push_str(&part.text);
            }
        }

        if accumulated_text.is_empty() {
            return Err(AppError::Internal(
                "No response received from Gemini API".to_string(),
            ));
        }

        // Parse the accumulated text as the expected type
        let parsed_response = serde_json::from_str::<T>(&accumulated_text).map_err(|e| {
            error!(
                "Failed to parse structured response: {}, Raw: {}",
                e, accumulated_text
            );
            AppError::Internal(format!("Failed to parse structured response: {}", e))
        })?;

        Ok((accumulated_text, parsed_response))
    }
}
