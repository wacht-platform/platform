use chrono::{Datelike, Utc};
use common::error::AppError;
use serde::{Deserialize, Serialize};

const GEMINI_API_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta/models";

#[derive(Debug, Clone)]
pub struct GeminiClient {
    api_key: String,
    model: String,
    client: reqwest::Client,
    deployment_id: Option<i64>,
    redis_client: Option<redis::Client>,
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
    #[serde(rename = "thoughtSignature")]
    pub thought_signature: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UsageMetadata {
    #[serde(rename = "promptTokenCount")]
    pub prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    pub candidates_token_count: u32,
    #[serde(rename = "totalTokenCount")]
    pub total_token_count: u32,
    #[serde(rename = "thoughtsTokenCount", default)]
    pub thoughts_token_count: Option<u32>,
}

impl GeminiClient {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            api_key,
            model: model.unwrap_or_else(|| "gemini-2.0-flash-exp".to_string()),
            client: reqwest::Client::new(),
            deployment_id: None,
            redis_client: None,
        }
    }

    pub fn with_billing(mut self, deployment_id: i64, redis_client: redis::Client) -> Self {
        self.deployment_id = Some(deployment_id);
        self.redis_client = Some(redis_client);
        self
    }

    pub async fn generate_structured_content<T>(&self, request_body: String) -> Result<(T, Option<String>), AppError>
    where
        T: for<'de> Deserialize<'de> + Serialize,
    {
        let url = format!("{}/{}:generateContent", GEMINI_API_BASE_URL, self.model);

        let mut last_error = None;
        for attempt in 1..=3 {
            if attempt > 1 {
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
                        Ok(bytes) => {
                            // Log raw response for debugging parse failures
                            let raw_response = String::from_utf8_lossy(&bytes);
                            
                            match serde_json::from_slice::<GeminiResponse>(&bytes) {
                            Ok(gemini_response) => {
                                let mut accumulated_text = String::new();
                                let mut thought_signature = None;
                                
                                for part in &gemini_response.candidates[0].content.parts {
                                    accumulated_text.push_str(&part.text);
                                    if let Some(sig) = &part.thought_signature {
                                        thought_signature = Some(sig.clone());
                                    }
                                }

                                if accumulated_text.is_empty() {
                                    last_error =
                                        Some("No response content from Gemini API".to_string());
                                    continue;
                                }

                                match serde_json::from_str::<T>(&accumulated_text) {
                                    Ok(parsed_response) => {
                                        if let Some(usage) = &gemini_response.usage_metadata {
                                            self.track_token_usage(usage).await;
                                        }
                                        return Ok((parsed_response, thought_signature));
                                    }
                                    Err(e) => {
                                        last_error = Some(format!("Failed to parse response: {e}"));
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Gemini API parse error: {}. Raw response (first 500 chars): {}",
                                    e,
                                    &raw_response.chars().take(500).collect::<String>()
                                );
                                last_error = Some(format!("Invalid API response format: {e}"));
                            }
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

    async fn track_token_usage(&self, usage: &UsageMetadata) {
        let Some(deployment_id) = self.deployment_id else { return };
        let Some(redis_client) = &self.redis_client else { return };

        if let Ok(mut conn) = redis_client.get_multiplexed_async_connection().await {
            let now = Utc::now();
            let period = format!("{}-{:02}", now.year(), now.month());
            let prefix = format!("billing:{}:deployment:{}", period, deployment_id);

            let input_tokens = usage.prompt_token_count as i64;
            let output_tokens = usage.candidates_token_count as i64
                + usage.thoughts_token_count.unwrap_or(0) as i64;

            let mut pipe = redis::pipe();
            pipe.atomic()
                .zincr(&format!("{}:metrics", prefix), "ai_tokens_input", input_tokens)
                .ignore()
                .zincr(&format!("{}:metrics", prefix), "ai_tokens_output", output_tokens)
                .ignore()
                .expire(&format!("{}:metrics", prefix), 5184000)
                .ignore()
                .zincr(&format!("billing:{}:dirty_deployments", period), deployment_id, input_tokens + output_tokens)
                .ignore()
                .expire(&format!("billing:{}:dirty_deployments", period), 5184000)
                .ignore();

            let _: Result<(), redis::RedisError> = pipe.query_async(&mut conn).await;
        }
    }


}
