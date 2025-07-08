use eventsource_client::{self as es, Client};
use futures::{Stream, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    ) -> Result<T, AppError>
    where
        T: for<'de> Deserialize<'de>,
    {
        let url = format!(
            "{}/{}:streamGenerateContent?alt=sse",
            GEMINI_API_BASE_URL, self.model
        );

        // The request_body is already a JSON string from the template
        let body = request_body;

        debug!("Sending request to Gemini API: {}", url);
        debug!("Request body: {}", body);

        let client = es::ClientBuilder::for_url(&url)
            .map_err(|e| AppError::Internal(format!("Failed to build client: {}", e)))?
            .header("x-goog-api-key", &self.api_key)
            .map_err(|e| AppError::Internal(format!("Failed to set API key header: {}", e)))?
            .header("Content-Type", "application/json")
            .map_err(|e| AppError::Internal(format!("Failed to set Content-Type header: {}", e)))?
            .method("POST".to_string())
            .body(body)
            .build();

        let mut stream = client.stream();
        let mut accumulated_text = String::new();

        while let Some(event) = stream
            .try_next()
            .await
            .map_err(|e| AppError::Internal(format!("Error streaming events: {}", e)))?
        {
            match event {
                es::SSE::Event(ev) => {
                    if ev.data.trim().is_empty() {
                        continue;
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
            }
        }

        if accumulated_text.is_empty() {
            return Err(AppError::Internal(
                "No response received from Gemini API".to_string(),
            ));
        }

        debug!("Accumulated response: {}", accumulated_text);

        // Parse the accumulated JSON
        serde_json::from_str::<T>(&accumulated_text)
            .map_err(|e| AppError::Internal(format!("Failed to parse structured response: {}", e)))
    }

    pub fn generate_structured_content_stream(
        &self,
        request_body: String,
    ) -> impl Stream<Item = Result<String, es::Error>> + '_ {
        let url = format!(
            "{}/{}:streamGenerateContent?alt=sse",
            GEMINI_API_BASE_URL, self.model
        );

        // The request_body is already a JSON string from the template
        let body = request_body;

        let client = es::ClientBuilder::for_url(&url)
            .unwrap()
            .header("x-goog-api-key", &self.api_key)
            .unwrap()
            .header("Content-Type", "application/json")
            .unwrap()
            .method("POST".to_string())
            .body(body)
            .build();

        client.stream().try_filter_map(move |event| async move {
            match event {
                es::SSE::Event(ev) => {
                    if ev.data.trim().is_empty() {
                        return Ok(None);
                    }

                    match serde_json::from_str::<GeminiStreamResponse>(&ev.data) {
                        Ok(response) => {
                            let mut chunk = String::new();
                            for candidate in &response.candidates {
                                for part in &candidate.content.parts {
                                    chunk.push_str(&part.text);
                                }
                            }

                            if !chunk.is_empty() {
                                Ok(Some(chunk))
                            } else {
                                Ok(None)
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse SSE data: {}, data: {}", e, ev.data);
                            Ok(None)
                        }
                    }
                }
                _ => Ok(None),
            }
        })
    }
}

// Helper function to create schema for common types
pub mod schema {
    use serde_json::{Value, json};

    pub fn string(description: Option<&str>) -> Value {
        let mut schema = json!({
            "type": "STRING"
        });
        if let Some(desc) = description {
            schema["description"] = json!(desc);
        }
        schema
    }

    pub fn number(description: Option<&str>) -> Value {
        let mut schema = json!({
            "type": "NUMBER"
        });
        if let Some(desc) = description {
            schema["description"] = json!(desc);
        }
        schema
    }

    pub fn boolean(description: Option<&str>) -> Value {
        let mut schema = json!({
            "type": "BOOLEAN"
        });
        if let Some(desc) = description {
            schema["description"] = json!(desc);
        }
        schema
    }

    pub fn array(items: Value, description: Option<&str>) -> Value {
        let mut schema = json!({
            "type": "ARRAY",
            "items": items
        });
        if let Some(desc) = description {
            schema["description"] = json!(desc);
        }
        schema
    }

    pub fn object(properties: Value, required: Vec<&str>, description: Option<&str>) -> Value {
        let mut schema = json!({
            "type": "OBJECT",
            "properties": properties,
            "required": required
        });
        if let Some(desc) = description {
            schema["description"] = json!(desc);
        }
        schema
    }

    pub fn enum_string(values: Vec<&str>, description: Option<&str>) -> Value {
        let mut schema = json!({
            "type": "STRING",
            "enum": values
        });
        if let Some(desc) = description {
            schema["description"] = json!(desc);
        }
        schema
    }
}
