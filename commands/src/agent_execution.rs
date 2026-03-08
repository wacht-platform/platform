use common::{HasAgentStorageProvider, HasIdProvider, HasNatsJetStreamProvider, error::AppError};
use dto::json::{AgentExecutionRequest, AgentExecutionType, NatsTaskMessage};
use models::{FileData, ImageData};

use crate::WriteToAgentStorageCommand;

const AGENT_EXECUTION_KV_BUCKET: &str = "agent_execution_kv";

fn sanitize_upload_filename(name: &str) -> Result<String, AppError> {
    let mut out = String::with_capacity(name.len());
    let mut prev_underscore = false;

    for ch in name.chars() {
        let is_allowed = ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-';
        if is_allowed {
            out.push(ch);
            prev_underscore = false;
        } else if !prev_underscore {
            out.push('_');
            prev_underscore = true;
        }
    }

    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        return Err(AppError::BadRequest("Invalid filename".to_string()));
    }
    Ok(trimmed.to_string())
}

/// Command to upload images to S3 storage via the agent storage gateway
/// Returns a vector of ImageData with relative URLs
pub struct UploadImagesToS3Command {
    deployment_id: i64,
    context_id: i64,
    images: Option<Vec<dto::json::agent_executor::ImageData>>,
}

impl UploadImagesToS3Command {
    pub fn new(
        deployment_id: i64,
        context_id: i64,
        images: Option<Vec<dto::json::agent_executor::ImageData>>,
    ) -> Self {
        Self {
            deployment_id,
            context_id,
            images,
        }
    }
}

impl UploadImagesToS3Command {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<Vec<ImageData>>, AppError>
    where
        D: HasAgentStorageProvider + HasIdProvider + ?Sized,
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
            let filename = format!("{}.{}", deps.id_provider().next_id()? as i64, file_extension);

            // S3 key: {deployment}/persistent/{context}/uploads/{filename}
            let key = format!(
                "{}/persistent/{}/uploads/{}",
                self.deployment_id, self.context_id, filename
            );

            // Upload to S3 via agent storage command
            let write_image_command = WriteToAgentStorageCommand::new(key, bytes.clone())
                .with_content_type(img.mime_type.clone());
            let storage_client = deps.agent_storage_provider()?;
            write_image_command
                .execute_with_deps(storage_client)
                .await?;

            uploaded.push(ImageData {
                mime_type: img.mime_type,
                url: format!("/uploads/{}", filename), // Relative path for agent filesystem
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
    context_id: i64,
    files: Option<Vec<dto::json::agent_executor::FileData>>,
}

impl UploadFilesToS3Command {
    pub fn new(
        deployment_id: i64,
        context_id: i64,
        files: Option<Vec<dto::json::agent_executor::FileData>>,
    ) -> Self {
        Self {
            deployment_id,
            context_id,
            files,
        }
    }
}

impl UploadFilesToS3Command {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Option<Vec<FileData>>, AppError>
    where
        D: HasAgentStorageProvider + HasIdProvider + ?Sized,
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
            let filename = format!(
                "{}_{}",
                deps.id_provider().next_id()? as i64,
                safe_filename
            );

            // S3 key: {deployment}/persistent/{context}/uploads/{filename}
            let key = format!(
                "{}/persistent/{}/uploads/{}",
                self.deployment_id, self.context_id, filename
            );

            // Upload to S3 via agent storage command
            let write_file_command = WriteToAgentStorageCommand::new(key, bytes.clone())
                .with_content_type(file.mime_type.clone());
            let storage_client = deps.agent_storage_provider()?;
            write_file_command
                .execute_with_deps(storage_client)
                .await?;

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

/// Command to publish an agent execution request to NATS
/// The worker will pick this up and execute the agent
pub struct PublishAgentExecutionCommand {
    request: AgentExecutionRequest,
}

pub struct SignalAgentExecutionCancellationCommand {
    context_id: i64,
}

impl SignalAgentExecutionCancellationCommand {
    pub fn new(context_id: i64) -> Self {
        Self { context_id }
    }
}

impl SignalAgentExecutionCancellationCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasNatsJetStreamProvider + HasIdProvider,
    {
        let marker_id = deps.id_provider().next_id().map_err(|e| {
            AppError::Internal(format!(
                "Failed to generate cancellation marker id for context {}: {}",
                self.context_id, e
            ))
        })? as i64;
        let jetstream = deps.nats_jetstream_provider();
        let kv = match jetstream.get_key_value(AGENT_EXECUTION_KV_BUCKET).await {
            Ok(store) => store,
            Err(_) => match jetstream
                .create_key_value(async_nats::jetstream::kv::Config {
                    bucket: AGENT_EXECUTION_KV_BUCKET.to_string(),
                    ..Default::default()
                })
                .await
            {
                Ok(store) => store,
                Err(create_error) => {
                    jetstream
                        .get_key_value(AGENT_EXECUTION_KV_BUCKET)
                        .await
                        .map_err(|get_error| {
                            AppError::Internal(format!(
                                "Failed to initialize cancellation KV bucket: create error={}, get error={}",
                                create_error, get_error
                            ))
                        })?
                }
            }};

        let marker = format!("cancel:{}", marker_id);
        kv.put(self.context_id.to_string(), marker.into())
            .await
            .map_err(|error| {
                AppError::Internal(format!(
                    "Failed to signal cancellation for context {}: {}",
                    self.context_id, error
                ))
            })?;

        Ok(())
    }
}

impl PublishAgentExecutionCommand {
    pub fn new(request: AgentExecutionRequest) -> Self {
        Self { request }
    }

    pub fn new_message(
        deployment_id: i64,
        context_id: i64,
        agent_id: Option<i64>,
        agent_name: Option<String>,
        conversation_id: i64,
    ) -> Self {
        Self {
            request: AgentExecutionRequest {
                deployment_id: deployment_id.to_string(),
                context_id: context_id.to_string(),
                agent_name,
                agent_id: agent_id.map(|id| id.to_string()),
                execution_type: AgentExecutionType::NewMessage {
                    conversation_id: conversation_id.to_string(),
                },
            },
        }
    }

    pub fn user_input_response(
        deployment_id: i64,
        context_id: i64,
        agent_id: Option<i64>,
        agent_name: Option<String>,
        conversation_id: i64,
    ) -> Self {
        Self {
            request: AgentExecutionRequest {
                deployment_id: deployment_id.to_string(),
                context_id: context_id.to_string(),
                agent_name,
                agent_id: agent_id.map(|id| id.to_string()),
                execution_type: AgentExecutionType::UserInputResponse {
                    conversation_id: conversation_id.to_string(),
                },
            },
        }
    }

    pub fn platform_function_result(
        deployment_id: i64,
        context_id: i64,
        agent_id: Option<i64>,
        agent_name: Option<String>,
        execution_id: String,
        result: serde_json::Value,
    ) -> Self {
        Self {
            request: AgentExecutionRequest {
                deployment_id: deployment_id.to_string(),
                context_id: context_id.to_string(),
                agent_name,
                agent_id: agent_id.map(|id| id.to_string()),
                execution_type: AgentExecutionType::PlatformFunctionResult {
                    execution_id,
                    result,
                },
            },
        }
    }
}

impl PublishAgentExecutionCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasNatsJetStreamProvider + HasIdProvider,
    {
        let task_id = deps
            .id_provider()
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate task id: {}", e)))?
            as i64;
        let jetstream = deps.nats_jetstream_provider();
        let task = NatsTaskMessage {
            task_id: format!("exec_{}", task_id),
            task_type: "agent.execution_request".to_string(),
            payload: serde_json::to_value(&self.request).map_err(|e| {
                AppError::Internal(format!("Failed to serialize execution request: {}", e))
            })?,
        };

        let payload = serde_json::to_vec(&task)
            .map_err(|e| AppError::Internal(format!("Failed to serialize task message: {}", e)))?;

        jetstream
            .publish("worker.tasks.agent.execution_request", payload.into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {}", e)))?;

        let agent_identifier = self
            .request
            .agent_id
            .map(|id| id.to_string())
            .or(self.request.agent_name.clone())
            .unwrap_or_else(|| "unknown".to_string());

        tracing::info!(
            "Published agent execution request for context {} (agent: {})",
            self.request.context_id,
            agent_identifier
        );

        Ok(())
    }
}
