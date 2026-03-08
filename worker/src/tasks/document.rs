use anyhow::Result;
use common::state::AppState;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessDocumentTask {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub document_id: i64,
}

pub async fn process_document_impl(
    deployment_id: i64,
    knowledge_base_id: i64,
    document_id: i64,
    app_state: &AppState,
) -> Result<String> {
    use commands::{ProcessDocumentCommand, ProcessDocumentDeps};

    info!(
        "Processing document {} in knowledge base {} for deployment {}",
        document_id, knowledge_base_id, deployment_id
    );

    let command = ProcessDocumentCommand::new(deployment_id, knowledge_base_id, document_id);

    let storage_client = app_state
        .agent_storage_client
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Agent storage client not configured"))?;

    command
        .execute_with_deps(ProcessDocumentDeps {
            acquirer: app_state.db_router.writer(),
            storage_client,
            text_processing_service: &app_state.text_processing_service,
            dispatch_batch: |dep_id, kb_id, batch_size| async move {
                commands::DispatchDocumentBatchTaskCommand::new(dep_id, kb_id, batch_size)
                    .execute_with_deps(&common::deps::from_app(app_state).nats())
                    .await
            },
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to process document: {}", e))
}
