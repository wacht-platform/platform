use chrono::Utc;
use common::{
    HasDbRouter, HasEmbeddingProvider, HasEncryptionProvider, HasIdProvider, HasNatsProvider,
    MemoryQueryFilters, connect_vector_store, error::AppError, insert_memory,
    load_memories_in_table, open_memory_table_in_connection,
    open_or_create_memory_table_in_connection, search_memories_full_text_in_table,
    search_memories_in_table,
};
use dto::json::agent_executor::{MemorySearchApproach, MemorySource, SearchDepth};
use dto::json::memory::MemoryCategory;
use models::MemoryRecord;

use crate::{
    DispatchVectorStoreMaintenanceTaskCommand, GenerateEmbeddingsCommand,
    ResolveDeploymentStorageCommand, VECTOR_STORE_MEMORY,
};

pub struct StoreMemoryCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub actor_id: Option<i64>,
    pub project_id: Option<i64>,
    pub thread_id: Option<i64>,
    pub execution_run_id: Option<i64>,
    pub owner_agent_id: Option<i64>,
    pub recorded_by_agent_id: Option<i64>,
    pub memory_scope: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub memory_category: MemoryCategory,
}

pub struct SaveAgentMemoryCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub thread_id: i64,
    pub execution_run_id: i64,
    pub actor_id: i64,
    pub project_id: i64,
    pub content: String,
    pub category: Option<String>,
    pub scope: Option<String>,
}

pub struct LoadAgentMemoryCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub thread_id: i64,
    pub actor_id: i64,
    pub project_id: i64,
    pub query: String,
    pub categories: Vec<MemoryCategory>,
    pub sources: Vec<MemorySource>,
    pub depth: Option<SearchDepth>,
    pub search_approach: MemorySearchApproach,
}

impl StoreMemoryCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<MemoryRecord, AppError>
    where
        D: HasDbRouter
            + HasEmbeddingProvider
            + HasEncryptionProvider
            + HasNatsProvider
            + HasIdProvider
            + ?Sized,
    {
        let now = Utc::now();

        let storage = ResolveDeploymentStorageCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;
        let lance_config = storage.vector_store_config();
        if !storage.vector_store_initialized {
            return Err(AppError::Validation(
                "Deployment vector store is not initialized. Re-save AI storage settings first."
                    .to_string(),
            ));
        }

        let conn = connect_vector_store(&lance_config).await?;
        let table = open_or_create_memory_table_in_connection(&conn).await?;

        let record = MemoryRecord {
            id: self.id,
            deployment_id: self.deployment_id,
            actor_id: self.actor_id,
            project_id: self.project_id,
            thread_id: self.thread_id,
            execution_run_id: self.execution_run_id,
            owner_agent_id: self.owner_agent_id,
            recorded_by_agent_id: self.recorded_by_agent_id,
            memory_scope: self.memory_scope,
            content: self.content,
            embedding: Some(self.embedding),
            memory_category: self.memory_category.to_string(),
            metadata: serde_json::json!({}),
            created_at: now,
            updated_at: now,
        };
        insert_memory(&table, &record).await?;

        DispatchVectorStoreMaintenanceTaskCommand::new(
            self.deployment_id,
            VECTOR_STORE_MEMORY.to_string(),
            format!("memory-{}", self.id),
        )
        .execute_with_deps(deps)
        .await?;

        Ok(record)
    }
}

impl SaveAgentMemoryCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<MemoryRecord, AppError>
    where
        D: HasDbRouter
            + HasEmbeddingProvider
            + HasEncryptionProvider
            + HasNatsProvider
            + HasIdProvider
            + ?Sized,
    {
        let category_str = self.category.as_deref().unwrap_or("working");
        let scope_str = self
            .scope
            .as_deref()
            .unwrap_or(models::memory::scope::THREAD);

        let category = MemoryCategory::from_str(category_str).unwrap_or(MemoryCategory::Working);

        let embeddings = GenerateEmbeddingsCommand::new(vec![self.content.clone()])
            .for_retrieval_document()
            .for_deployment(self.deployment_id)
            .execute_with_deps(deps)
            .await?;

        let embedding = embeddings
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("Failed to generate embedding".to_string()))?;

        let (actor_id, project_id, thread_id, owner_agent_id, memory_scope) = match scope_str {
            models::memory::scope::ACTOR => (
                Some(self.actor_id),
                None,
                None,
                None,
                models::memory::scope::ACTOR.to_string(),
            ),
            models::memory::scope::PROJECT => (
                Some(self.actor_id),
                Some(self.project_id),
                None,
                None,
                models::memory::scope::PROJECT.to_string(),
            ),
            models::memory::scope::AGENT => (
                None,
                None,
                None,
                Some(self.agent_id),
                models::memory::scope::AGENT.to_string(),
            ),
            _ => (
                Some(self.actor_id),
                Some(self.project_id),
                Some(self.thread_id),
                Some(self.agent_id),
                models::memory::scope::THREAD.to_string(),
            ),
        };

        StoreMemoryCommand {
            id: deps.id_provider().next_id()? as i64,
            deployment_id: self.deployment_id,
            actor_id,
            project_id,
            thread_id,
            execution_run_id: Some(self.execution_run_id),
            owner_agent_id,
            recorded_by_agent_id: Some(self.agent_id),
            memory_scope,
            content: self.content,
            embedding,
            memory_category: category,
        }
        .execute_with_deps(deps)
        .await
    }
}

impl LoadAgentMemoryCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<MemoryRecord>, AppError>
    where
        D: HasDbRouter
            + HasEmbeddingProvider
            + HasEncryptionProvider
            + HasNatsProvider
            + HasIdProvider
            + ?Sized,
    {
        let storage = ResolveDeploymentStorageCommand::new(self.deployment_id)
            .execute_with_deps(deps)
            .await?;
        if !storage.vector_store_initialized {
            return Err(AppError::Validation(
                "Deployment vector store is not initialized. Re-save AI storage settings first."
                    .to_string(),
            ));
        }

        let conn = connect_vector_store(&storage.vector_store_config()).await?;
        let Some(table) = open_memory_table_in_connection(&conn).await? else {
            return Ok(Vec::new());
        };

        let limit = match self.depth.unwrap_or(SearchDepth::Moderate) {
            SearchDepth::Shallow => 20,
            SearchDepth::Moderate => 50,
            SearchDepth::Deep => 100,
        };
        let query = self.query.trim().to_string();

        if query.is_empty() {
            return load_recent_memories_from_sources(
                &table,
                self.deployment_id,
                self.thread_id,
                self.actor_id,
                self.project_id,
                self.agent_id,
                &self.sources,
                &self.categories,
                limit,
            )
            .await;
        }

        let filters = build_memory_query_filters(
            self.thread_id,
            self.actor_id,
            self.project_id,
            self.agent_id,
            &self.sources,
            &self.categories,
        );

        match self.search_approach {
            MemorySearchApproach::Semantic => {
                let embedding = build_query_embedding(deps, self.deployment_id, &query).await?;
                search_memories_in_table(&table, self.deployment_id, &embedding, &filters, limit)
                    .await
            }
            MemorySearchApproach::FullText => {
                search_memories_full_text_in_table(
                    &table,
                    self.deployment_id,
                    &query,
                    &filters,
                    limit,
                )
                .await
            }
            MemorySearchApproach::Hybrid => {
                let embedding = build_query_embedding(deps, self.deployment_id, &query).await?;
                let semantic = search_memories_in_table(
                    &table,
                    self.deployment_id,
                    &embedding,
                    &filters,
                    limit,
                )
                .await?;
                let text = search_memories_full_text_in_table(
                    &table,
                    self.deployment_id,
                    &query,
                    &filters,
                    limit,
                )
                .await?;
                Ok(merge_unique_memories(vec![semantic, text], limit))
            }
        }
    }
}

async fn build_query_embedding<D>(
    deps: &D,
    deployment_id: i64,
    query: &str,
) -> Result<Vec<f32>, AppError>
where
    D: HasDbRouter
        + HasEmbeddingProvider
        + HasEncryptionProvider
        + HasNatsProvider
        + HasIdProvider
        + ?Sized,
{
    let embeddings = GenerateEmbeddingsCommand::new(vec![query.to_string()])
        .for_retrieval_query()
        .for_deployment(deployment_id)
        .execute_with_deps(deps)
        .await?;

    embeddings
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("Failed to generate query embedding".to_string()))
}

fn build_memory_query_filters(
    thread_id: i64,
    actor_id: i64,
    project_id: i64,
    agent_id: i64,
    sources: &[MemorySource],
    categories: &[MemoryCategory],
) -> MemoryQueryFilters {
    MemoryQueryFilters {
        actor_id: sources.contains(&MemorySource::Actor).then_some(actor_id),
        project_id: sources
            .contains(&MemorySource::Project)
            .then_some(project_id),
        thread_id: sources.contains(&MemorySource::Thread).then_some(thread_id),
        agent_id: sources.contains(&MemorySource::Agent).then_some(agent_id),
        categories: Some(
            categories
                .iter()
                .map(|category| category.to_string())
                .collect::<Vec<_>>(),
        ),
    }
}

async fn load_recent_memories_from_sources(
    table: &lancedb::Table,
    deployment_id: i64,
    thread_id: i64,
    actor_id: i64,
    project_id: i64,
    agent_id: i64,
    sources: &[MemorySource],
    categories: &[MemoryCategory],
    limit: usize,
) -> Result<Vec<MemoryRecord>, AppError> {
    let deduped_sources = dedupe_sources(sources);
    if deduped_sources.is_empty() {
        return Ok(Vec::new());
    }

    let per_source_limit = std::cmp::max(1, limit.div_ceil(deduped_sources.len()));
    let mut groups = Vec::new();

    for source in deduped_sources {
        let base_filter = match source {
            MemorySource::Thread => format!(
                "embedding IS NOT NULL AND thread_id = {} AND memory_scope = '{}'",
                thread_id,
                models::memory::scope::THREAD
            ),
            MemorySource::Project => format!(
                "embedding IS NOT NULL AND project_id = {} AND memory_scope = '{}'",
                project_id,
                models::memory::scope::PROJECT
            ),
            MemorySource::Actor => format!(
                "embedding IS NOT NULL AND actor_id = {} AND memory_scope = '{}'",
                actor_id,
                models::memory::scope::ACTOR
            ),
            MemorySource::Agent => format!(
                "embedding IS NOT NULL AND owner_agent_id = {} AND memory_scope = '{}'",
                agent_id,
                models::memory::scope::AGENT
            ),
        };

        groups.push(
            load_memories_in_table(
                table,
                deployment_id,
                &append_memory_category_filter(base_filter, categories),
                per_source_limit,
            )
            .await?,
        );
    }

    Ok(merge_unique_memories(groups, limit))
}

fn dedupe_sources(sources: &[MemorySource]) -> Vec<MemorySource> {
    let mut deduped = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for source in sources {
        if seen.insert(*source) {
            deduped.push(*source);
        }
    }

    deduped
}

fn append_memory_category_filter(base_filter: String, categories: &[MemoryCategory]) -> String {
    if categories.is_empty() {
        return base_filter;
    }

    let joined = categories
        .iter()
        .map(|category| format!("'{}'", category.to_string().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{base_filter} AND memory_category IN ({joined})")
}

fn merge_unique_memories(groups: Vec<Vec<MemoryRecord>>, limit: usize) -> Vec<MemoryRecord> {
    let mut merged = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for group in groups {
        for memory in group {
            if seen.insert(memory.id) {
                merged.push(memory);
            }
        }
    }

    merged.truncate(limit);
    merged
}
