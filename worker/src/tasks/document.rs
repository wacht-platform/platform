use anyhow::Result;
use common::state::AppState;
use serde::{Deserialize, Serialize};

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
    use commands::ProcessDocumentCommand;
    let command_deps = common::deps::from_app(app_state).db().enc().nats();

    let command = ProcessDocumentCommand::new(deployment_id, knowledge_base_id, document_id);

    command
        .execute_with_deps(&command_deps)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to process document: {}", e))
}
