use aws_sdk_s3::primitives::ByteStream;
use common::error::AppError;
use common::state::AppState;

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
        let client = app_state
            .agent_storage_client
            .as_ref()
            .ok_or_else(|| AppError::Internal("Agent storage client not configured".to_string()))?;

        let mut request = client
            .put_object()
            .bucket("wacht-agents")
            .key(&self.key)
            .body(ByteStream::from(self.body));

        if let Some(ct) = self.content_type {
            request = request.content_type(ct);
        }

        request
            .send()
            .await
            .map_err(|e| AppError::S3(e.to_string()))?;

        Ok(self.key)
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
