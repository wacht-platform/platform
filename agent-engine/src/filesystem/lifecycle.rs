use super::AgentFilesystem;
use crate::sandbox::SandboxHandle;
use common::error::AppError;
use common::state::AppState;
use std::sync::Arc;

impl AgentFilesystem {
    pub fn new(
        app_state: &AppState,
        deployment_id: &str,
        thread_id: &str,
        sandbox_handle: Arc<dyn SandboxHandle>,
    ) -> Result<Self, AppError> {
        let deployment_id_num = deployment_id.parse::<i64>().map_err(|e| {
            AppError::Internal(format!(
                "Invalid deployment id '{}' for filesystem: {}",
                deployment_id, e
            ))
        })?;

        Ok(Self {
            deployment_id: deployment_id_num,
            app_state: app_state.clone(),
            thread_id: thread_id.to_string(),
            read_paths: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashSet::new(),
            )),
            sandbox_handle,
        })
    }
}
