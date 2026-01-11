use common::error::AppError;
use common::state::AppState;
use dto::json::{AgentExecutionRequest, AgentExecutionType, NatsTaskMessage};
use models::ImageData;

use crate::{Command, WriteToAgentStorageCommand};

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

impl Command for UploadImagesToS3Command {
    type Output = Option<Vec<ImageData>>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
            let filename = format!("{}.{}", app_state.sf.next_id()?, file_extension);

            // S3 key: {deployment}/persistent/{context}/uploads/{filename}
            let key = format!(
                "{}/persistent/{}/uploads/{}",
                self.deployment_id, self.context_id, filename
            );

            // Upload to S3 via agent storage command
            WriteToAgentStorageCommand::new(key, bytes.clone())
                .with_content_type(img.mime_type.clone())
                .execute(app_state)
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

/// Command to publish an agent execution request to NATS
/// The worker will pick this up and execute the agent
pub struct PublishAgentExecutionCommand {
    request: AgentExecutionRequest,
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

impl Command for PublishAgentExecutionCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let task = NatsTaskMessage {
            task_id: format!("exec_{}", app_state.sf.next_id()?),
            task_type: "agent.execution_request".to_string(),
            payload: serde_json::to_value(&self.request).map_err(|e| {
                AppError::Internal(format!("Failed to serialize execution request: {}", e))
            })?,
        };

        let payload = serde_json::to_vec(&task)
            .map_err(|e| AppError::Internal(format!("Failed to serialize task message: {}", e)))?;

        app_state
            .nats_jetstream
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
