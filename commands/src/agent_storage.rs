use aws_sdk_s3::primitives::ByteStream;
use common::error::AppError;
use common::state::AppState;
use tracing::{info, error, debug};

use crate::Command;

pub struct WriteToAgentStorageCommand {
    pub key: String,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

impl WriteToAgentStorageCommand {
    pub fn new(key: String, body: Vec<u8>) -> Self {
        Self {
            key,
            body,
            content_type: None,
        }
    }

    pub fn with_content_type(mut self, content_type: String) -> Self {
        self.content_type = Some(content_type);
        self
    }
}

impl Command for WriteToAgentStorageCommand {
    type Output = String;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        info!("[AgentStorage] Starting file upload to agent storage");
        debug!("[AgentStorage] Upload details - key: {}, content_type: {:?}, body_size: {} bytes",
            self.key, self.content_type, self.body.len());

        let client = app_state
            .agent_storage_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("Agent storage client not configured".to_string()))?;

        info!("[AgentStorage] Agent storage client is configured");

        let bucket = "wacht-agents";
        debug!("[AgentStorage] Target bucket: {}", bucket);

        let mut request = client
            .put_object()
            .bucket(bucket)
            .key(&self.key)
            .body(ByteStream::from(self.body));

        if let Some(ct) = self.content_type {
            debug!("[AgentStorage] Setting content-type: {}", ct);
            request = request.content_type(ct);
        }

        info!("[AgentStorage] Sending S3 put_object request...");

        match request.send().await {
            Ok(response) => {
                info!("[AgentStorage] Upload successful! ETag: {:?}", response.e_tag());
                debug!("[AgentStorage] Response: {:?}", response);
                Ok(self.key)
            }
            Err(e) => {
                error!("[AgentStorage] S3 upload failed!");
                error!("[AgentStorage] Error details: {:?}", e);
                error!("[AgentStorage] Error type: {}", std::any::type_name_of_val(&e));
                error!("[AgentStorage] Error message: {}", e);

                // Try to get more error details
                let err_msg = e.to_string();
                if err_msg.contains("dispatch") {
                    error!("[AgentStorage] DISPATCH FAILURE - This usually means:");
                    error!("[AgentStorage]  1. The S3 endpoint URL is incorrect or unreachable");
                    error!("[AgentStorage]  2. The S3 service is not running");
                    error!("[AgentStorage]  3. Network connectivity issue");
                    error!("[AgentStorage]  4. TLS/SSL certificate issues");
                } else if err_msg.contains("credentials") {
                    error!("[AgentStorage] CREDENTIALS ERROR - Check AGENT_STORAGE_ACCESS_KEY and AGENT_STORAGE_SECRET_KEY");
                } else if err_msg.contains("NoSuchBucket") {
                    error!("[AgentStorage] BUCKET NOT FOUND - Bucket '{}' does not exist", bucket);
                }

                Err(AppError::S3(e.to_string()))
            }
        }
    }
}

pub struct DeleteFromAgentStorageCommand {
    pub key: String,
}

impl DeleteFromAgentStorageCommand {
    pub fn new(key: String) -> Self {
        Self { key }
    }
}

impl Command for DeleteFromAgentStorageCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let client = app_state
            .agent_storage_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("Agent storage client not configured".to_string()))?;

        client
            .delete_object()
            .bucket("wacht-agents")
            .key(&self.key)
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        Ok(())
    }
}

pub struct DeletePrefixFromAgentStorageCommand {
    pub prefix: String,
}

impl DeletePrefixFromAgentStorageCommand {
    pub fn new(prefix: String) -> Self {
        Self { prefix }
    }
}

impl Command for DeletePrefixFromAgentStorageCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let client = app_state
            .agent_storage_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("Agent storage client not configured".to_string()))?;

        let list_result = client
            .list_objects_v2()
            .bucket("wacht-agents")
            .prefix(&self.prefix)
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        if let Some(objects) = list_result.contents {
            for obj in objects {
                if let Some(key) = obj.key {
                    client
                        .delete_object()
                        .bucket("wacht-agents")
                        .key(&key)
                        .send()
                        .await
                        .map_err(|e| AppError::S3(e.to_string()))?;
                }
            }
        }

        Ok(())
    }
}
