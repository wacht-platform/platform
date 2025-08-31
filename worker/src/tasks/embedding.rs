use anyhow::Result;
use common::state::AppState;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDocumentBatchTask {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub batch_size: usize,
}

pub async fn process_document_batch_impl(
    deployment_id: i64,
    knowledge_base_id: i64,
    batch_size: usize,
    app_state: &AppState,
) -> Result<String> {
    use commands::{Command, ProcessDocumentBatchCommand};

    info!(
        "Processing batch of up to {} pending documents for knowledge base {} in deployment {}",
        batch_size, knowledge_base_id, deployment_id
    );

    let command = ProcessDocumentBatchCommand::new(deployment_id, knowledge_base_id, batch_size);

    command
        .execute(app_state)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to process document batch: {}", e))
}
