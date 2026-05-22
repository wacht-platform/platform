use common::ResultExt;
use common::{
    HasDbRouter, HasEncryptionProvider, HasIdProvider, HasNatsJetStreamProvider, error::AppError,
};
use models::{FileData, ImageData};

use crate::WriteToDeploymentStorageCommand;

fn sanitize_upload_filename(name: &str) -> Result<String, AppError> {
    common::sanitize_filename(name)
        .ok_or_else(|| AppError::BadRequest("Invalid filename".to_string()))
}

/// Command to upload images to deployment storage
/// Returns a vector of ImageData with relative URLs
pub struct UploadImagesToS3Command {
    deployment_id: i64,
    thread_id: i64,
    images: Option<Vec<dto::json::agent_executor::ImageData>>,
}

impl UploadImagesToS3Command {
    pub fn new(
        deployment_id: i64,
        thread_id: i64,
        images: Option<Vec<dto::json::agent_executor::ImageData>>,
    ) -> Self {
        Self {
            deployment_id,
            thread_id,
            images,
        }
    }
}

impl UploadImagesToS3Command {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<Vec<ImageData>>, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider + HasIdProvider + ?Sized,
    {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let Some(imgs) = self.images else {
            return Ok(None);
        };

        let mut uploaded = Vec::new();

        for img in imgs {
            // Decode base64 image data
            let bytes = STANDARD
                .decode(&img.data)
                .map_err(|e| AppError::BadRequest(format!("Invalid base64 image data: {}", e)))?;

            // Get file extension from mime type
            let file_extension = img.mime_type.split('/').next_back().unwrap_or("png");
            let filename = format!(
                "{}.{}",
                deps.id_provider().next_id()? as i64,
                file_extension
            );

            // S3 key: {deployment}/persistent/{thread}/uploads/{filename}
            let key = format!(
                "{}/persistent/{}/uploads/{}",
                self.deployment_id, self.thread_id, filename
            );

            // Upload to deployment storage
            let write_image_command =
                WriteToDeploymentStorageCommand::new(self.deployment_id, key, bytes.clone())
                    .with_content_type(img.mime_type.clone());
            write_image_command.execute_with_deps(deps).await?;

            uploaded.push(ImageData {
                mime_type: img.mime_type,
                url: format!("/uploads/{}", filename),
                size_bytes: Some(bytes.len() as u64),
            });
        }

        if uploaded.is_empty() {
            Ok(None)
        } else {
            Ok(Some(uploaded))
        }
    }
}

/// Command to upload generic files to S3 storage
/// Returns a vector of FileData with relative URLs
pub struct UploadFilesToS3Command {
    deployment_id: i64,
    thread_id: i64,
    files: Option<Vec<dto::json::agent_executor::FileData>>,
}

impl UploadFilesToS3Command {
    pub fn new(
        deployment_id: i64,
        thread_id: i64,
        files: Option<Vec<dto::json::agent_executor::FileData>>,
    ) -> Self {
        Self {
            deployment_id,
            thread_id,
            files,
        }
    }
}

impl UploadFilesToS3Command {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<Vec<FileData>>, AppError>
    where
        D: HasDbRouter + HasEncryptionProvider + HasIdProvider + ?Sized,
    {
        use base64::{Engine, engine::general_purpose::STANDARD};

        let Some(files) = self.files else {
            return Ok(None);
        };

        let mut uploaded = Vec::new();

        for file in files {
            // Decode base64 file data
            let bytes = STANDARD
                .decode(&file.data)
                .map_err(|e| AppError::BadRequest(format!("Invalid base64 file data: {}", e)))?;

            // Generate unique filename with original name preserved
            let safe_filename = sanitize_upload_filename(&file.filename)?;
            let filename = format!("{}_{}", deps.id_provider().next_id()? as i64, safe_filename);

            // S3 key: {deployment}/persistent/{thread}/uploads/{filename}
            let key = format!(
                "{}/persistent/{}/uploads/{}",
                self.deployment_id, self.thread_id, filename
            );

            // Upload to deployment storage
            let write_file_command =
                WriteToDeploymentStorageCommand::new(self.deployment_id, key, bytes.clone())
                    .with_content_type(file.mime_type.clone());
            write_file_command.execute_with_deps(deps).await?;

            uploaded.push(FileData {
                filename: file.filename,
                mime_type: file.mime_type,
                url: format!("/uploads/{}", filename),
                size_bytes: Some(bytes.len() as u64),
            });
        }

        if uploaded.is_empty() {
            Ok(None)
        } else {
            Ok(Some(uploaded))
        }
    }
}

pub struct AdvanceThreadExecutionTokenCommand {
    thread_id: i64,
}

impl AdvanceThreadExecutionTokenCommand {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<String, AppError>
    where
        D: HasNatsJetStreamProvider + HasIdProvider + ?Sized,
    {
        let token = deps
            .id_provider()
            .next_id()
            .map_err_internal("Failed to generate execution token")?
            .to_string();
        write_execution_watch_key(
            deps.nats_jetstream_provider(),
            &self.thread_id.to_string(),
            &token,
        )
        .await?;
        Ok(token)
    }
}

pub async fn write_execution_watch_key(
    jetstream: &async_nats::jetstream::Context,
    key: &str,
    token: &str,
) -> Result<(), AppError> {
    let kv = match jetstream.get_key_value("agent_execution_kv").await {
        Ok(store) => store,
        Err(_) => jetstream
            .create_key_value(async_nats::jetstream::kv::Config {
                bucket: "agent_execution_kv".to_string(),
                ..Default::default()
            })
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to initialize execution token KV bucket: {}",
                    e
                ))
            })?,
    };
    kv.put(key.to_string(), token.as_bytes().to_vec().into())
        .await
        .map_err(|e| {
            AppError::Internal(format!("Failed to set execution watch key {}: {}", key, e))
        })?;
    Ok(())
}
