use super::ToolExecutor;
use common::error::AppError;
use dto::json::agent_executor::{LoadMemoryParams, SaveMemoryParams};
use serde_json::Value;

impl ToolExecutor {
    pub(super) async fn execute_load_memory(
        &self,
        tool: &models::AiTool,
        params: LoadMemoryParams,
    ) -> Result<Value, AppError> {
        let thread = self.ctx.get_thread().await?;
        let memories = commands::LoadAgentMemoryCommand {
            deployment_id: self.agent().deployment_id,
            agent_id: self.agent().id,
            thread_id: self.thread_id(),
            actor_id: thread.actor_id,
            project_id: thread.project_id,
            query: params.query,
            categories: params.categories,
            sources: params.sources,
            depth: params.depth,
            search_approach: params.search_approach,
        }
        .execute_with_deps(self.app_state())
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "count": memories.len(),
            "memories": memories.into_iter().map(|memory| serde_json::json!({
                "memory_id": memory.id.to_string(),
                "content": memory.content,
                "memory_category": memory.memory_category,
                "memory_scope": memory.memory_scope,
                "created_at": memory.created_at.to_rfc3339(),
                "updated_at": memory.updated_at.to_rfc3339(),
            })).collect::<Vec<_>>()
        }))
    }

    pub(super) async fn execute_save_memory(
        &self,
        tool: &models::AiTool,
        params: SaveMemoryParams,
    ) -> Result<Value, AppError> {
        let thread = self.ctx.get_thread().await?;
        let category = params.category.clone();
        let scope = params.scope.clone();
        let memory = commands::SaveAgentMemoryCommand {
            deployment_id: self.agent().deployment_id,
            agent_id: self.agent().id,
            thread_id: self.thread_id(),
            execution_run_id: self.ctx.execution_run_id,
            actor_id: thread.actor_id,
            project_id: thread.project_id,
            content: params.content,
            category,
            scope,
        }
        .execute_with_deps(self.app_state())
        .await?;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "message": "Memory saved successfully",
            "memory_id": memory.id.to_string(),
            "category": memory.memory_category,
            "scope": memory.memory_scope,
            "created_at": memory.created_at.to_rfc3339(),
            "updated_at": memory.updated_at.to_rfc3339()
        }))
    }
}
