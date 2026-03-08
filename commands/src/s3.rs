use aws_sdk_s3::primitives::{ByteStream, SdkBody};
use common::{HasS3Client, error::AppError};
use serde_json::json;

pub struct UploadToCdnCommand {
    pub file_path: String,
    pub body: Vec<u8>,
}

impl UploadToCdnCommand {
    pub fn new(file_path: String, body: Vec<u8>) -> Self {
        Self { file_path, body }
    }
}

impl UploadToCdnCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<String, AppError>
    where
        D: HasS3Client + ?Sized,
    {
        deps.s3_client()
            .put_object()
            .bucket(std::env::var("R2_CDN_BUCKET").expect("R2_CDN_BUCKET must be set"))
            .key(&self.file_path)
            .body(ByteStream::new(SdkBody::from(self.body)))
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        let client = reqwest::Client::new();
        let _ = client
            .post("https://api.cloudflare.com/client/v4/zones/90930ab39928937ca4d0c4aba3b03126/purge_cache")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", std::env::var("CLOUDFLARE_API_KEY").expect("CLOUDFLARE_API_KEY must be set")))
            .json(&json!({
                "files": [
                    format!("https://cdn.wacht.services/{}", self.file_path)
                ]
            }))
            .send()
            .await;

        Ok(format!("https://cdn.wacht.services/{}", self.file_path))
    }
}

pub struct UploadToKnowledgeBaseBucketCommand {
    pub file_path: String,
    pub body: Vec<u8>,
}

impl UploadToKnowledgeBaseBucketCommand {
    pub fn new(file_path: String, body: Vec<u8>) -> Self {
        Self { file_path, body }
    }
}

impl UploadToKnowledgeBaseBucketCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<String, AppError>
    where
        D: HasS3Client + ?Sized,
    {
        deps.s3_client()
            .put_object()
            .bucket("wacht-knowledge-base")
            .key(&self.file_path)
            .body(ByteStream::new(SdkBody::from(self.body)))
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        Ok(format!(
            "https://wacht-knowledge-base.r2.cloudflarestorage.com/{}",
            self.file_path
        ))
    }
}
