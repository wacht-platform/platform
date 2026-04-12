use common::{
    HasDbRouter, HasEncryptionProvider, HasIdProvider, HasNatsJetStreamProvider, error::AppError,
};
use dto::json::{
    AgentExecutionRequest, AgentExecutionType, NatsTaskMessage, ThreadScheduleRequest,
};
use models::{FileData, ImageData, ThreadEvent};

use crate::WriteToDeploymentStorageCommand;

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

/// Command to publish an agent execution request to NATS
/// The worker will pick this up and execute the agent
pub struct PublishAgentExecutionCommand {
    request: AgentExecutionRequest,
}

pub struct PublishThreadScheduleCommand {
    request: ThreadScheduleRequest,
}

pub struct AdvanceThreadExecutionTokenCommand {
    thread_id: i64,
}

impl PublishAgentExecutionCommand {
    pub fn new(request: AgentExecutionRequest) -> Self {
        Self { request }
    }

    pub fn from_thread_event(
        thread_event: &ThreadEvent,
        agent_id: Option<i64>,
    ) -> Result<Self, AppError> {
        match thread_event.event_type.as_str() {
            "user_message_received" => {
                let conversation_id = thread_event
                    .conversation_payload()
                    .map(|payload| payload.conversation_id)
                    .ok_or_else(|| {
                        AppError::BadRequest(
                            "user_message_received event is missing conversation_id".to_string(),
                        )
                    })?;

                Ok(Self::new_message(
                    thread_event.deployment_id,
                    thread_event.thread_id,
                    Some(thread_event.id),
                    agent_id,
                    conversation_id,
                ))
            }
            "approval_response_received" => {
                let payload = thread_event
                    .approval_response_received_payload()
                    .ok_or_else(|| {
                        AppError::BadRequest(
                            "approval_response_received event has invalid payload".to_string(),
                        )
                    })?;
                let request_message_id = payload.request_message_id.to_string();
                let approvals = payload
                    .approvals
                    .into_iter()
                    .map(|approval| dto::json::deployment::ToolApprovalSelection {
                        tool_name: approval.tool_name,
                        mode: approval.mode,
                    })
                    .collect();

                Ok(Self::approval_response(
                    thread_event.deployment_id,
                    thread_event.thread_id,
                    Some(thread_event.id),
                    agent_id,
                    request_message_id,
                    approvals,
                ))
            }
            "task_routing" | "assignment_execution" | "assignment_outcome_review" => {
                Ok(Self::thread_event(
                    thread_event.deployment_id,
                    thread_event.thread_id,
                    thread_event.id,
                    agent_id,
                ))
            }
            other => Err(AppError::BadRequest(format!(
                "Unsupported thread event type for execution publish: {}",
                other
            ))),
        }
    }

    pub fn new_message(
        deployment_id: i64,
        thread_id: i64,
        thread_event_id: Option<i64>,
        agent_id: Option<i64>,
        conversation_id: i64,
    ) -> Self {
        Self {
            request: AgentExecutionRequest {
                deployment_id: deployment_id.to_string(),
                thread_id: thread_id.to_string(),
                thread_event_id: thread_event_id.map(|id| id.to_string()),
                agent_id: agent_id.map(|id| id.to_string()),
                execution_type: AgentExecutionType::NewMessage {
                    conversation_id: conversation_id.to_string(),
                },
            },
        }
    }

    pub fn approval_response(
        deployment_id: i64,
        thread_id: i64,
        thread_event_id: Option<i64>,
        agent_id: Option<i64>,
        request_message_id: String,
        approvals: Vec<dto::json::deployment::ToolApprovalSelection>,
    ) -> Self {
        Self {
            request: AgentExecutionRequest {
                deployment_id: deployment_id.to_string(),
                thread_id: thread_id.to_string(),
                thread_event_id: thread_event_id.map(|id| id.to_string()),
                agent_id: agent_id.map(|id| id.to_string()),
                execution_type: AgentExecutionType::ApprovalResponse {
                    request_message_id,
                    approvals,
                },
            },
        }
    }

    pub fn thread_event(
        deployment_id: i64,
        thread_id: i64,
        thread_event_id: i64,
        agent_id: Option<i64>,
    ) -> Self {
        Self {
            request: AgentExecutionRequest {
                deployment_id: deployment_id.to_string(),
                thread_id: thread_id.to_string(),
                thread_event_id: Some(thread_event_id.to_string()),
                agent_id: agent_id.map(|id| id.to_string()),
                execution_type: AgentExecutionType::ThreadEvent {
                    event_id: thread_event_id.to_string(),
                },
            },
        }
    }
}

impl PublishThreadScheduleCommand {
    pub fn new(deployment_id: i64, thread_id: i64) -> Self {
        Self {
            request: ThreadScheduleRequest {
                deployment_id: deployment_id.to_string(),
                thread_id: thread_id.to_string(),
            },
        }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasNatsJetStreamProvider + HasIdProvider + ?Sized,
    {
        let task_id = deps
            .id_provider()
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate task id: {}", e)))?
            as i64;
        let jetstream = deps.nats_jetstream_provider();
        let task = NatsTaskMessage {
            task_id: task_id.to_string(),
            task_type: "agent.thread_schedule".to_string(),
            payload: serde_json::to_value(&self.request).map_err(|e| {
                AppError::Internal(format!(
                    "Failed to serialize thread schedule request: {}",
                    e
                ))
            })?,
        };

        let payload = serde_json::to_vec(&task)
            .map_err(|e| AppError::Internal(format!("Failed to serialize task message: {}", e)))?;

        jetstream
            .publish("worker.tasks.agent.thread_schedule", payload.into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish to NATS: {}", e)))?;

        tracing::info!(
            "Published thread schedule request for thread {}",
            self.request.thread_id
        );

        Ok(())
    }
}

impl AdvanceThreadExecutionTokenCommand {
    pub fn new(thread_id: i64) -> Self {
        Self { thread_id }
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<String, AppError>
    where
        D: HasNatsJetStreamProvider + HasIdProvider + ?Sized,
    {
        let execution_token = deps
            .id_provider()
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate execution token: {}", e)))?
            as i64;

        let jetstream = deps.nats_jetstream_provider();
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

        let token = execution_token.to_string();
        kv.put(self.thread_id.to_string(), token.clone().into())
            .await
            .map_err(|e| {
                AppError::Internal(format!(
                    "Failed to advance execution token for thread {}: {}",
                    self.thread_id, e
                ))
            })?;

        Ok(token)
    }
}

impl PublishAgentExecutionCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasNatsJetStreamProvider + HasIdProvider + ?Sized,
    {
        let task_id = deps
            .id_provider()
            .next_id()
            .map_err(|e| AppError::Internal(format!("Failed to generate task id: {}", e)))?
            as i64;
        let jetstream = deps.nats_jetstream_provider();
        let task = NatsTaskMessage {
            task_id: task_id.to_string(),
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

        let agent_identifier = self.request.agent_id.clone().unwrap_or_else(|| "unknown".to_string());

        tracing::info!(
            "Published agent execution request for thread {} (agent: {})",
            self.request.thread_id,
            agent_identifier
        );

        Ok(())
    }
}
