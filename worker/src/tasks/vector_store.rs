use anyhow::Result;
use common::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorStoreMaintenanceTask {
    pub deployment_id: i64,
    pub store_name: String,
}

pub async fn maintain_vector_store_impl(
    deployment_id: i64,
    store_name: String,
    app_state: &AppState,
) -> Result<String> {
    let command_deps = common::deps::from_app(app_state).db().enc();
    commands::MaintainVectorStoreIndexCommand::new(deployment_id, store_name)
        .execute_with_deps(&command_deps)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to maintain vector store index: {}", e))
}
