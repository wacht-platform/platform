use aws_sdk_s3::primitives::{ByteStream, SdkBody};
use common::{HasS3Provider, error::AppError};

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
        D: HasS3Provider + ?Sized,
    {
        deps.s3_provider()
            .put_object()
            .bucket(std::env::var("R2_CDN_BUCKET").expect("R2_CDN_BUCKET must be set"))
            .key(&self.file_path)
            .body(ByteStream::new(SdkBody::from(self.body)))
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        Ok(format!("https://cdn.wacht.services/{}", self.file_path))
    }
}

pub struct DeleteFromCdnCommand {
    pub file_path: String,
}

impl DeleteFromCdnCommand {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasS3Provider + ?Sized,
    {
        deps.s3_provider()
            .delete_object()
            .bucket(std::env::var("R2_CDN_BUCKET").expect("R2_CDN_BUCKET must be set"))
            .key(&self.file_path)
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        Ok(())
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
        D: HasS3Provider + ?Sized,
    {
        deps.s3_provider()
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
