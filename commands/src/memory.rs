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
    ResolveDeploymentStorageCommand, VECTOR_STORE_MEMORY, resolve_deployment_embedding_dimension,
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
    pub metadata: serde_json::Value,
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
    pub observation: Option<String>,
    pub signals: Vec<String>,
    pub related: Vec<String>,
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
        let embedding_dimension =
            resolve_deployment_embedding_dimension(deps, self.deployment_id).await?;

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
            metadata: self.metadata,
            created_at: now,
            updated_at: now,
        };
        insert_memory(&table, &record, embedding_dimension).await?;

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
        let category_str = self.category.as_deref().unwrap_or("semantic");
        let scope_str = self
            .scope
            .as_deref()
            .unwrap_or(models::memory::scope::PROJECT);

        let category = MemoryCategory::from_str(category_str).unwrap_or(MemoryCategory::Semantic);

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
            _ => (
                Some(self.actor_id),
                Some(self.project_id),
                Some(self.thread_id),
                Some(self.agent_id),
                models::memory::scope::THREAD.to_string(),
            ),
        };

        let mut metadata_obj = serde_json::Map::new();
        if let Some(observation) = self.observation.as_ref().map(|s| s.trim()) {
            if !observation.is_empty() {
                metadata_obj.insert(
                    "observation".to_string(),
                    serde_json::Value::String(observation.to_string()),
                );
            }
        }
        if !self.signals.is_empty() {
            metadata_obj.insert(
                "signals".to_string(),
                serde_json::Value::Array(
                    self.signals
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
        if !self.related.is_empty() {
            metadata_obj.insert(
                "related".to_string(),
                serde_json::Value::Array(
                    self.related
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
        }

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
            metadata: serde_json::Value::Object(metadata_obj),
        }
        .execute_with_deps(deps)
        .await
    }
}

pub struct UpdateAgentMemoryCommand {
    pub deployment_id: i64,
    pub memory_id: i64,
    pub actor_id: i64,
    pub project_id: i64,
    pub thread_id: i64,
    pub content: Option<String>,
    pub category: Option<String>,
    pub scope: Option<String>,
    pub observation: Option<String>,
    pub signals: Option<Vec<String>>,
    pub related: Option<Vec<String>>,
}

impl UpdateAgentMemoryCommand {
    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<MemoryRecord, AppError>
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
        let lance_config = storage.vector_store_config();
        if !storage.vector_store_initialized {
            return Err(AppError::Validation(
                "Deployment vector store is not initialized.".to_string(),
            ));
        }

        let conn = connect_vector_store(&lance_config).await?;
        let table = open_or_create_memory_table_in_connection(&conn).await?;
        let embedding_dimension =
            resolve_deployment_embedding_dimension(deps, self.deployment_id).await?;

        let filter = format!("id = {}", self.memory_id);
        let mut existing = load_memories_in_table(
            &table,
            self.deployment_id,
            &filter,
            1,
            embedding_dimension,
        )
        .await?;
        let existing = existing.pop().ok_or_else(|| {
            AppError::NotFound(format!("Memory {} not found", self.memory_id))
        })?;

        let scope_changed = self
            .scope
            .as_deref()
            .map(|s| s != existing.memory_scope)
            .unwrap_or(false);
        if scope_changed {
            return Err(AppError::Validation(
                "Updating memory_scope is not supported; re-save the memory in the new scope instead."
                    .to_string(),
            ));
        }

        let new_content = self.content.clone().unwrap_or_else(|| existing.content.clone());
        let content_changed = new_content != existing.content;
        let embedding = if content_changed {
            let embeddings = GenerateEmbeddingsCommand::new(vec![new_content.clone()])
                .for_retrieval_document()
                .for_deployment(self.deployment_id)
                .execute_with_deps(deps)
                .await?;
            embeddings.into_iter().next().ok_or_else(|| {
                AppError::Internal("Failed to generate embedding for updated memory".to_string())
            })?
        } else {
            existing.embedding.clone().ok_or_else(|| {
                AppError::Internal(
                    "Existing memory has no embedding to preserve during update".to_string(),
                )
            })?
        };

        let category = self
            .category
            .as_deref()
            .map(|c| {
                MemoryCategory::from_str(c).ok_or_else(|| {
                    AppError::Validation(format!("Unknown memory category '{}'", c))
                })
            })
            .transpose()?
            .map(|c| c.to_string())
            .unwrap_or(existing.memory_category.clone());

        let mut metadata_obj = existing
            .metadata
            .as_object()
            .cloned()
            .unwrap_or_else(serde_json::Map::new);

        if let Some(observation) = self.observation.as_ref() {
            let trimmed = observation.trim();
            if trimmed.is_empty() {
                metadata_obj.remove("observation");
            } else {
                metadata_obj.insert(
                    "observation".to_string(),
                    serde_json::Value::String(trimmed.to_string()),
                );
            }
        }
        if let Some(signals) = self.signals.as_ref() {
            if signals.is_empty() {
                metadata_obj.remove("signals");
            } else {
                metadata_obj.insert(
                    "signals".to_string(),
                    serde_json::Value::Array(
                        signals
                            .iter()
                            .map(|s| serde_json::Value::String(s.clone()))
                            .collect(),
                    ),
                );
            }
        }
        if let Some(related) = self.related.as_ref() {
            if related.is_empty() {
                metadata_obj.remove("related");
            } else {
                metadata_obj.insert(
                    "related".to_string(),
                    serde_json::Value::Array(
                        related
                            .iter()
                            .map(|s| serde_json::Value::String(s.clone()))
                            .collect(),
                    ),
                );
            }
        }

        let now = Utc::now();
        let updated_record = MemoryRecord {
            id: existing.id,
            deployment_id: existing.deployment_id,
            actor_id: existing.actor_id,
            project_id: existing.project_id,
            thread_id: existing.thread_id,
            execution_run_id: existing.execution_run_id,
            owner_agent_id: existing.owner_agent_id,
            recorded_by_agent_id: existing.recorded_by_agent_id,
            memory_scope: existing.memory_scope.clone(),
            content: new_content,
            embedding: Some(embedding),
            memory_category: category,
            metadata: serde_json::Value::Object(metadata_obj),
            created_at: existing.created_at,
            updated_at: now,
        };

        table
            .delete(&format!("id = {}", self.memory_id))
            .await
            .map_err(common::vector_store::map_vector_store_error)?;
        insert_memory(&table, &updated_record, embedding_dimension).await?;

        DispatchVectorStoreMaintenanceTaskCommand::new(
            self.deployment_id,
            VECTOR_STORE_MEMORY.to_string(),
            format!("memory-{}", self.memory_id),
        )
        .execute_with_deps(deps)
        .await
        .ok();

        Ok(updated_record)
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
        let embedding_dimension =
            resolve_deployment_embedding_dimension(deps, self.deployment_id).await?;

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
                &self.sources,
                &self.categories,
                limit,
                embedding_dimension,
            )
            .await;
        }

        let filters = build_memory_query_filters(
            self.thread_id,
            self.actor_id,
            self.project_id,
            &self.sources,
            &self.categories,
        );

        match self.search_approach {
            MemorySearchApproach::Semantic => {
                let embedding = build_query_embedding(deps, self.deployment_id, &query).await?;
                search_memories_in_table(
                    &table,
                    self.deployment_id,
                    &embedding,
                    &filters,
                    limit,
                    embedding_dimension,
                )
                .await
            }
            MemorySearchApproach::FullText => {
                search_memories_full_text_in_table(
                    &table,
                    self.deployment_id,
                    &query,
                    &filters,
                    limit,
                    embedding_dimension,
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
                    embedding_dimension,
                )
                .await?;
                let text = search_memories_full_text_in_table(
                    &table,
                    self.deployment_id,
                    &query,
                    &filters,
                    limit,
                    embedding_dimension,
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
    sources: &[MemorySource],
    categories: &[MemoryCategory],
) -> MemoryQueryFilters {
    MemoryQueryFilters {
        actor_id: sources.contains(&MemorySource::Actor).then_some(actor_id),
        project_id: sources
            .contains(&MemorySource::Project)
            .then_some(project_id),
        thread_id: sources.contains(&MemorySource::Thread).then_some(thread_id),
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
    sources: &[MemorySource],
    categories: &[MemoryCategory],
    limit: usize,
    embedding_dimension: i32,
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
        };

        groups.push(
            load_memories_in_table(
                table,
                deployment_id,
                &append_memory_category_filter(base_filter, categories),
                per_source_limit,
                embedding_dimension,
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
