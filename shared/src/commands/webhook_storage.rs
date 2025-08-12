use aws_sdk_s3::primitives::ByteStream;
use chrono::Utc;
use serde_json::Value;
use zstd::stream::{decode_all, encode_all};
use futures::future::join_all;

use crate::{
    error::AppError,
    state::AppState,
};

use super::Command;

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

        // Generate S3 key with date-based partitioning and snowflake ID
        let key = format!(
            "webhooks/{}/{}.json.zst",
            Utc::now().format("%Y/%m/%d"),
            app_state.sf.next_id().unwrap()
        );

        // Serialize and compress payload
        let json_bytes = serde_json::to_vec(&self.payload)
            .map_err(|e| AppError::Internal(format!("Failed to serialize payload: {}", e)))?;
        
        let compressed = encode_all(json_bytes.as_slice(), 3)
            .map_err(|e| AppError::Internal(format!("Failed to compress payload: {}", e)))?;

        // Upload to S3
        app_state.s3_client
            .put_object()
            .bucket(&bucket)
            .key(&key)
            .body(ByteStream::from(compressed))
            .content_encoding("zstd")
            .content_type("application/json")
            .metadata("original_size", json_bytes.len().to_string())
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to upload to S3: {}", e)))?;

        Ok(format!("s3://{}/{}", bucket, key))
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

        // Parse S3 URI
        let key = self.s3_key
            .strip_prefix(&format!("s3://{}/", bucket))
            .ok_or_else(|| AppError::BadRequest(format!("Invalid S3 key: {}", self.s3_key)))?;

        // Get object from S3
        let response = app_state.s3_client
            .get_object()
            .bucket(&bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to get object from S3: {}", e)))?;

        // Collect body bytes
        let body = response
            .body
            .collect()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read S3 object body: {}", e)))?
            .into_bytes();

        // Decompress
        let decompressed = decode_all(&body[..])
            .map_err(|e| AppError::Internal(format!("Failed to decompress payload: {}", e)))?;

        // Parse JSON
        serde_json::from_slice(&decompressed)
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

        let json_bytes = serde_json::to_vec(&wrapper)
            .map_err(|e| AppError::Internal(format!("Failed to serialize failed delivery: {}", e)))?;
        
        let compressed = encode_all(json_bytes.as_slice(), 3)
            .map_err(|e| AppError::Internal(format!("Failed to compress failed delivery: {}", e)))?;

        app_state.s3_client
            .put_object()
            .bucket(&bucket)
            .key(&key)
            .body(ByteStream::from(compressed))
            .content_encoding("zstd")
            .content_type("application/json")
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to upload failed delivery to S3: {}", e)))?;

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

        let key = self.s3_key
            .strip_prefix(&format!("s3://{}/", bucket))
            .ok_or_else(|| AppError::BadRequest(format!("Invalid S3 key: {}", self.s3_key)))?;

        app_state.s3_client
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
                let key = s3_key.strip_prefix(&format!("s3://{}/", bucket))
                    .unwrap_or(&s3_key);
                
                let result = async {
                    let object = s3_client
                        .get_object()
                        .bucket(&bucket)
                        .key(key)
                        .send()
                        .await
                        .map_err(|e| format!("Failed to get object: {}", e))?;
                    
                    let body = object.body.collect().await
                        .map_err(|e| format!("Failed to read body: {}", e))?
                        .into_bytes();
                    
                    // Decompress
                    let decompressed = decode_all(&body[..])
                        .map_err(|e| format!("Failed to decompress: {}", e))?;
                    
                    // Parse JSON
                    serde_json::from_slice::<Value>(&decompressed)
                        .map_err(|e| format!("Failed to parse JSON: {}", e))
                }.await;
                
                (s3_key, result)
            }
        });
        
        let results = join_all(futures).await;
        Ok(results)
    }
}