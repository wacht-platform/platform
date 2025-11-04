use aws_sdk_s3::primitives::ByteStream;
use chrono::Utc;
use futures::future::join_all;
use serde_json::Value;

use crate::Command;
use common::error::AppError;
use common::state::AppState;

#[derive(Debug)]
pub struct StoreWebhookPayloadCommand {
    pub payload: Value,
    pub bucket: Option<String>,
}

impl StoreWebhookPayloadCommand {
    pub fn new(payload: Value) -> Self {
        Self {
            payload,
            bucket: None,
        }
    }

    pub fn with_bucket(mut self, bucket: String) -> Self {
        self.bucket = Some(bucket);
        self
    }
}

impl Command for StoreWebhookPayloadCommand {
    type Output = String;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let bucket = self.bucket.unwrap_or_else(|| {
            std::env::var("WEBHOOK_BUCKET").unwrap_or_else(|_| "webhooks".to_string())
        });

        let key = format!(
            "webhooks/{}/{}.json",
            Utc::now().format("%Y/%m/%d"),
            app_state.sf.next_id().unwrap()
        );

        let json_bytes = serde_json::to_vec(&self.payload)
            .map_err(|e| AppError::Internal(format!("Failed to serialize payload: {}", e)))?;

        tracing::debug!(
            "Uploading webhook payload to S3: bucket={}, key={}",
            bucket,
            key
        );

        app_state
            .s3_client
            .put_object()
            .bucket(&bucket)
            .key(&key)
            .body(ByteStream::from(json_bytes.clone()))
            .content_type("application/json")
            .metadata("original_size", json_bytes.len().to_string())
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    "S3 upload failed - bucket: {}, key: {}, error: {}",
                    bucket,
                    key,
                    e
                );
                AppError::Internal(format!("Failed to upload to S3: {:?}", e))
            })?;

        Ok(key)
    }
}

#[derive(Debug)]
pub struct RetrieveWebhookPayloadCommand {
    pub s3_key: String,
    pub bucket: Option<String>,
}

impl RetrieveWebhookPayloadCommand {
    pub fn new(s3_key: String) -> Self {
        Self {
            s3_key,
            bucket: None,
        }
    }

    pub fn with_bucket(mut self, bucket: String) -> Self {
        self.bucket = Some(bucket);
        self
    }
}

impl Command for RetrieveWebhookPayloadCommand {
    type Output = Value;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let bucket = self.bucket.unwrap_or_else(|| {
            std::env::var("WEBHOOK_BUCKET").unwrap_or_else(|_| "webhooks".to_string())
        });

        let key = &self.s3_key;

        let response = app_state
            .s3_client
            .get_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    "Failed to get object from S3: bucket={}, key={}, error={}",
                    bucket,
                    key,
                    e
                );
                AppError::Internal(format!("Failed to get object from S3: {}", e))
            })?;

        let body = response
            .body
            .collect()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read S3 object body: {}", e)))?
            .into_bytes();

        serde_json::from_slice(&body)
            .map_err(|e| AppError::Internal(format!("Failed to parse JSON payload: {}", e)))
    }
}

#[derive(Debug)]
pub struct StoreFailedWebhookDeliveryCommand {
    pub delivery_id: i64,
    pub payload: Value,
    pub error: String,
    pub bucket: Option<String>,
}

impl StoreFailedWebhookDeliveryCommand {
    pub fn new(delivery_id: i64, payload: Value, error: String) -> Self {
        Self {
            delivery_id,
            payload,
            error,
            bucket: None,
        }
    }

    pub fn with_bucket(mut self, bucket: String) -> Self {
        self.bucket = Some(bucket);
        self
    }
}

impl Command for StoreFailedWebhookDeliveryCommand {
    type Output = String;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let bucket = self.bucket.unwrap_or_else(|| {
            std::env::var("WEBHOOK_BUCKET").unwrap_or_else(|_| "webhooks".to_string())
        });

        // Store failed deliveries in a separate prefix for debugging
        let key = format!(
            "webhooks/failed/{}/{}.json.zst",
            Utc::now().format("%Y/%m/%d"),
            self.delivery_id
        );

        let wrapper = serde_json::json!({
            "delivery_id": self.delivery_id,
            "error": self.error,
            "failed_at": Utc::now().to_rfc3339(),
            "payload": self.payload
        });

        let json_bytes = serde_json::to_vec(&wrapper).map_err(|e| {
            AppError::Internal(format!("Failed to serialize failed delivery: {}", e))
        })?;

        app_state
            .s3_client
            .put_object()
            .bucket(&bucket)
            .key(&key)
            .body(ByteStream::from(json_bytes))
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to upload failed delivery to S3: {}", e))
            })?;

        Ok(format!("s3://{}/{}", bucket, key))
    }
}

#[derive(Debug)]
pub struct DeleteWebhookPayloadCommand {
    pub s3_key: String,
    pub bucket: Option<String>,
}

impl DeleteWebhookPayloadCommand {
    pub fn new(s3_key: String) -> Self {
        Self {
            s3_key,
            bucket: None,
        }
    }

    pub fn with_bucket(mut self, bucket: String) -> Self {
        self.bucket = Some(bucket);
        self
    }
}

impl Command for DeleteWebhookPayloadCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let bucket = self.bucket.unwrap_or_else(|| {
            std::env::var("WEBHOOK_BUCKET").unwrap_or_else(|_| "webhooks".to_string())
        });

        let key = self
            .s3_key
            .strip_prefix(&format!("s3://{}/", bucket))
            .ok_or_else(|| AppError::BadRequest(format!("Invalid S3 key: {}", self.s3_key)))?;

        app_state
            .s3_client
            .delete_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete from S3: {}", e)))?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct BatchRetrieveWebhookPayloadsCommand {
    pub s3_keys: Vec<String>,
}

impl Command for BatchRetrieveWebhookPayloadsCommand {
    type Output = Vec<(String, Result<Value, String>)>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let bucket = std::env::var("WEBHOOK_BUCKET").unwrap_or_else(|_| "webhooks".to_string());

        // Process keys in parallel
        let futures = self.s3_keys.into_iter().map(|s3_key| {
            let s3_client = app_state.s3_client.clone();
            let bucket = bucket.clone();

            async move {
                let key = s3_key
                    .strip_prefix(&format!("s3://{}/", bucket))
                    .unwrap_or(&s3_key);

                let result = async {
                    let object = s3_client
                        .get_object()
                        .bucket(&bucket)
                        .key(key)
                        .send()
                        .await
                        .map_err(|e| format!("Failed to get object: {}", e))?;

                    let body = object
                        .body
                        .collect()
                        .await
                        .map_err(|e| format!("Failed to read body: {}", e))?
                        .into_bytes();

                    // Parse JSON directly (no decompression)
                    serde_json::from_slice::<Value>(&body)
                        .map_err(|e| format!("Failed to parse JSON: {}", e))
                }
                .await;

                (s3_key, result)
            }
        });

        let results = join_all(futures).await;
        Ok(results)
    }
}
