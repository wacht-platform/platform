mod delegation;
mod filesystem;
mod knowledge;
mod memory;
mod task_graph;
mod web;

use super::ToolExecutor;
use common::error::AppError;
use dto::json::agent_executor::ToolCallRequest;
use models::AiTool;
use serde_json::Value;

impl ToolExecutor {
    pub(super) async fn execute_internal_tool_request(
        &self,
        tool: &AiTool,
        request: &ToolCallRequest,
        filesystem: &crate::filesystem::AgentFilesystem,
        shell: &crate::filesystem::shell::ShellExecutor,
    ) -> Result<Value, AppError> {
        match request {
            ToolCallRequest::ReadImage { params, .. } => {
                self.execute_read_image(tool, filesystem, params.clone())
                    .await
            }
            ToolCallRequest::ReadFile { params, .. } => {
                self.execute_read_file(tool, filesystem, params.clone())
                    .await
            }
            ToolCallRequest::WriteFile { params, .. } => {
                self.execute_write_file(tool, filesystem, params.clone())
                    .await
            }
            ToolCallRequest::EditFile { params, .. } => {
                self.execute_edit_file(tool, filesystem, params.clone())
                    .await
            }
            ToolCallRequest::ExecuteCommand { params, .. } => {
                self.execute_command(tool, shell, params.clone()).await
            }
            ToolCallRequest::WebSearch { params, .. } => {
                self.execute_web_search_tool(tool, params.clone()).await
            }
            ToolCallRequest::UrlContent { params, .. } => {
                self.execute_url_content_tool(tool, params.clone()).await
            }
            ToolCallRequest::SearchKnowledgebase { params, .. } => {
                self.execute_search_knowledgebase_tool(params.clone()).await
            }
            ToolCallRequest::LoadMemory { params, .. } => {
                self.execute_load_memory(tool, params.clone()).await
            }
            ToolCallRequest::SaveMemory { params, .. } => {
                self.execute_save_memory(tool, params.clone()).await
            }
            ToolCallRequest::Sleep { params, .. } => self.execute_sleep(tool, params.clone()).await,
            ToolCallRequest::SnapshotExecutionState { .. } => Err(AppError::BadRequest(
                "snapshot_execution_state must be executed by the agent runtime".to_string(),
            )),
            ToolCallRequest::ListThreads { params, .. } => {
                self.execute_list_threads(tool, params.clone()).await
            }
            ToolCallRequest::CreateThread { params, .. } => {
                self.execute_create_thread(tool, params.clone()).await
            }
            ToolCallRequest::UpdateThread { params, .. } => {
                self.execute_update_thread(tool, params.clone()).await
            }
            ToolCallRequest::TaskGraphAddNode { params, .. } => {
                self.execute_task_graph_add_node(tool, params.clone()).await
            }
            ToolCallRequest::TaskGraphAddDependency { params, .. } => {
                self.execute_task_graph_add_dependency(tool, params.clone())
                    .await
            }
            ToolCallRequest::TaskGraphMarkInProgress { params, .. } => {
                self.execute_task_graph_mark_in_progress(tool, params.clone())
                    .await
            }
            ToolCallRequest::TaskGraphCompleteNode { params, .. } => {
                self.execute_task_graph_complete_node(tool, params.clone())
                    .await
            }
            ToolCallRequest::TaskGraphFailNode { params, .. } => {
                self.execute_task_graph_fail_node(tool, params.clone())
                    .await
            }
            ToolCallRequest::TaskGraphMarkCompleted { params, .. } => {
                self.execute_task_graph_mark_completed(tool, params.clone(), filesystem)
                    .await
            }
            ToolCallRequest::TaskGraphMarkFailed { params, .. } => {
                self.execute_task_graph_mark_failed(tool, params.clone(), filesystem)
                    .await
            }
            ToolCallRequest::SearchTools { .. }
            | ToolCallRequest::LoadTools { .. }
            | ToolCallRequest::CreateProjectTask { .. }
            | ToolCallRequest::UpdateProjectTask { .. }
            | ToolCallRequest::AssignProjectTask { .. }
            | ToolCallRequest::External(_) => Err(AppError::BadRequest(
                "Unsupported request kind for internal tool execution".to_string(),
            )),
        }
    }
}
