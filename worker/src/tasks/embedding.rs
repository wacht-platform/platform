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
    use commands::{ProcessDocumentBatchCommand, ProcessDocumentBatchDeps};

    info!(
        "Processing batch of up to {} pending documents for knowledge base {} in deployment {}",
        batch_size, knowledge_base_id, deployment_id
    );

    let command = ProcessDocumentBatchCommand::new(deployment_id, knowledge_base_id, batch_size);

    command
        .execute_with_deps(ProcessDocumentBatchDeps {
            acquirer: app_state.db_router.writer(),
            generate_embeddings: |texts| async move {
                let client = reqwest::Client::new();
                let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| {
                    common::error::AppError::Internal("GEMINI_API_KEY is not set".to_string())
                })?;
                let model = std::env::var("GEMINI_EMBEDDING_MODEL")
                    .unwrap_or_else(|_| "models/gemini-embedding-001".to_string());
                commands::GenerateEmbeddingsCommand::new(texts)
                    .execute_with_deps(commands::EmbeddingApiDeps {
                        client: &client,
                        api_key: &api_key,
                        model: &model,
                    })
                    .await
            },
        })
        .await
        .map_err(|e| anyhow::anyhow!("Failed to process document batch: {}", e))
}
