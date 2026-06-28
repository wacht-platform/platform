use super::ToolExecutor;
use commands::{GenerateEmbeddingsCommand, ResolveDeploymentStorageCommand};
use common::error::AppError;
use common::{
    connect_vector_store, open_memory_table_in_connection, search_memories_in_table,
    MemoryQueryFilters,
};
use dto::json::agent_executor::{LoadMemoryParams, SaveMemoryParams, UpdateMemoryParams};
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
                "observation": memory.metadata.get("observation"),
                "signals": memory.metadata.get("signals"),
                "related": memory.metadata.get("related"),
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
        let deployment_id = self.agent().deployment_id;
        let agent_id = self.agent().id;
        let thread_id = self.thread_id();
        let actor_id = thread.actor_id;
        let project_id = thread.project_id;

        // Dedup pre-check: search similar memories unless confirmed
        if !params.confirmed {
            if let Some(similar) = self
                .find_similar_memories(
                    deployment_id,
                    thread_id,
                    actor_id,
                    project_id,
                    &params.content,
                )
                .await?
            {
                return Ok(serde_json::json!({
                    "success": true,
                    "tool": tool.name,
                    "saved": false,
                    "message": "Similar existing memories found. Merge into one of these via `revise_memory(<id>, ...)` to consolidate, or call `save_memory` again with `confirmed: true` to save a distinct new entry.",
                    "similar_memories": similar,
                }));
            }
        }

        // No similar found (or confirmed): save as usual
        let memory = commands::SaveAgentMemoryCommand {
            deployment_id,
            agent_id,
            thread_id,
            execution_run_id: self.ctx.execution_run_id,
            actor_id,
            project_id,
            content: params.content,
            category: params.category,
            scope: params.scope,
            observation: params.observation,
            signals: params.signals,
            related: params.related,
        }
        .execute_with_deps(self.app_state())
        .await?;

        crate::executor::context::memory_context::invalidate_startup_memory_cache(
            self.app_state(),
            self.thread_id(),
        )
        .await;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "saved": true,
            "message": "Memory saved successfully",
            "memory_id": memory.id.to_string(),
            "category": memory.memory_category,
            "scope": memory.memory_scope,
            "created_at": memory.created_at.to_rfc3339(),
            "updated_at": memory.updated_at.to_rfc3339()
        }))
    }

    /// Search for semantically similar memories. Returns `Some(Vec)` with close
    /// matches if any found above threshold, `None` if the search couldn't run
    /// (no vector store) or found nothing.
    async fn find_similar_memories(
        &self,
        deployment_id: i64,
        thread_id: i64,
        actor_id: i64,
        project_id: i64,
        content: &str,
    ) -> Result<Option<Vec<serde_json::Value>>, AppError> {
        // Resolve storage to check if vector store is initialized
        let storage = ResolveDeploymentStorageCommand::new(deployment_id)
            .execute_with_deps(self.app_state())
            .await?;
        if !storage.vector_store_initialized {
            return Ok(None);
        }
        let lance_config = storage.vector_store_config();
        let conn = connect_vector_store(&lance_config).await?;
        let Some(table) = open_memory_table_in_connection(&conn).await? else {
            return Ok(None);
        };
        let embedding_dimension = commands::resolve_deployment_embedding_dimension(
            self.app_state(),
            deployment_id,
        )
        .await?;

        // Generate query embedding from the content
        let embeddings = GenerateEmbeddingsCommand::new(vec![content.to_string()])
            .for_retrieval_query()
            .for_deployment(deployment_id)
            .execute_with_deps(self.app_state())
            .await?;
        let Some(query_embedding) = embeddings.into_iter().next() else {
            return Ok(None);
        };

        // Search within relevant scopes
        let filters = MemoryQueryFilters {
            actor_id: Some(actor_id),
            project_id: Some(project_id),
            thread_id: Some(thread_id),
            categories: None,
        };
        // Fetch enough candidates that cosine-distance filtering is meaningful
        let results = search_memories_in_table(
            &table,
            deployment_id,
            &query_embedding,
            &filters,
            20,
            embedding_dimension,
        )
        .await?;

        if results.is_empty() {
            return Ok(None);
        }

        let similar: Vec<serde_json::Value> = results
            .into_iter()
            .map(|m| {
                serde_json::json!({
                    "memory_id": m.id.to_string(),
                    "content": m.content,
                    "memory_category": m.memory_category,
                    "memory_scope": m.memory_scope,
                    "observation": m.metadata.get("observation"),
                    "signals": m.metadata.get("signals"),
                    "related": m.metadata.get("related"),
                    "created_at": m.created_at.to_rfc3339(),
                })
            })
            .collect();

        Ok(Some(similar))
    }

    pub(super) async fn execute_update_memory(
        &self,
        tool: &models::AiTool,
        params: UpdateMemoryParams,
    ) -> Result<Value, AppError> {
        let thread = self.ctx.get_thread().await?;
        let memory_id = params.memory_id.parse::<i64>().map_err(|_| {
            AppError::BadRequest(format!(
                "No memory exists with id '{}'. Did you mean to create a new memory entry? If so, call `save_memory` — `revise_memory` only edits an existing memory by its numeric id.",
                params.memory_id
            ))
        })?;

        let memory = match (commands::UpdateAgentMemoryCommand {
            deployment_id: self.agent().deployment_id,
            memory_id,
            actor_id: thread.actor_id,
            project_id: thread.project_id,
            thread_id: self.thread_id(),
            content: params.content,
            category: params.category,
            scope: params.scope,
            observation: params.observation,
            signals: params.signals,
            related: params.related,
        }
        .execute_with_deps(self.app_state())
        .await)
        {
            Ok(memory) => memory,
            Err(AppError::NotFound(_)) => {
                return Err(AppError::NotFound(format!(
                    "No memory exists with id '{}'. Did you mean to create a new memory entry? If so, call `save_memory` — `revise_memory` only edits an existing memory.",
                    memory_id
                )));
            }
            Err(other) => return Err(other),
        };

        crate::executor::context::memory_context::invalidate_startup_memory_cache(
            self.app_state(),
            self.thread_id(),
        )
        .await;

        Ok(serde_json::json!({
            "success": true,
            "tool": tool.name,
            "message": "Memory updated",
            "memory_id": memory.id.to_string(),
            "category": memory.memory_category,
            "scope": memory.memory_scope,
            "observation": memory.metadata.get("observation"),
            "signals": memory.metadata.get("signals"),
            "related": memory.metadata.get("related"),
            "updated_at": memory.updated_at.to_rfc3339(),
        }))
    }
}
