use std::collections::HashSet;
use std::sync::Arc;

use arrow_array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, Int64Array, RecordBatch, StringArray,
    TimestampMicrosecondArray,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef, TimeUnit};
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use lancedb::index::Index;
use lancedb::index::scalar::BTreeIndexBuilder;
use lancedb::index::scalar::FullTextSearchQuery;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::{Connection, DistanceType, Table};
use models::error::AppError;
use models::{MemoryRecord, memory};

use crate::vector_store::{VectorStoreConfig, map_vector_store_error};

const MEMORY_TABLE: &str = "memories";
const MEMORY_EMBEDDING_DIMENSION: i32 = 1536;

#[derive(Debug, Clone, Default)]
pub struct MemoryQueryFilters {
    pub actor_id: Option<i64>,
    pub project_id: Option<i64>,
    pub thread_id: Option<i64>,
    pub agent_id: Option<i64>,
    pub categories: Option<Vec<String>>,
}

pub async fn initialize_memory_table(config: &VectorStoreConfig) -> Result<(), AppError> {
    let conn = crate::vector_store::connect_vector_store(config).await?;
    let table = open_or_create_memory_table_in_connection(&conn).await?;
    ensure_memory_indices(&table).await
}

pub async fn open_or_create_memory_table_in_connection(
    conn: &Connection,
) -> Result<Table, AppError> {
    crate::vector_store::open_or_create_table_in_connection(conn, MEMORY_TABLE, memory_schema())
        .await
}

pub async fn open_memory_table_in_connection(conn: &Connection) -> Result<Option<Table>, AppError> {
    crate::vector_store::open_table_in_connection(conn, MEMORY_TABLE).await
}

pub async fn insert_memory(table: &Table, record: &MemoryRecord) -> Result<(), AppError> {
    let batch = build_memory_record_batch(std::slice::from_ref(record))?;
    table
        .add(batch)
        .execute()
        .await
        .map_err(map_vector_store_error)?;
    Ok(())
}

pub async fn count_indexable_memory_rows(config: &VectorStoreConfig) -> Result<usize, AppError> {
    crate::vector_store::count_rows_with_filter(
        config,
        MEMORY_TABLE,
        "id > 0 AND embedding IS NOT NULL",
    )
    .await
}

pub async fn memory_vector_index_exists(config: &VectorStoreConfig) -> Result<bool, AppError> {
    crate::vector_store::vector_index_exists(config, MEMORY_TABLE, "embedding").await
}

pub async fn create_memory_vector_index(config: &VectorStoreConfig) -> Result<(), AppError> {
    crate::vector_store::create_auto_vector_index(
        config,
        MEMORY_TABLE,
        memory_schema(),
        "embedding",
    )
    .await
}

pub async fn optimize_memory_vector_index(config: &VectorStoreConfig) -> Result<(), AppError> {
    crate::vector_store::optimize_vector_index(config, MEMORY_TABLE).await
}

pub async fn get_startup_memories_in_table(
    table: &Table,
    deployment_id: i64,
    thread_id: i64,
    actor_id: i64,
    limit: usize,
) -> Result<Vec<MemoryRecord>, AppError> {
    let filter = format!(
        "id > 0 AND ((thread_id = {} AND memory_scope = '{}') OR (actor_id = {} AND memory_scope = '{}'))",
        thread_id,
        memory::scope::THREAD,
        actor_id,
        memory::scope::ACTOR
    );
    load_memories_in_table(table, deployment_id, &filter, limit).await
}

pub async fn load_memories_in_table(
    table: &Table,
    deployment_id: i64,
    filter: &str,
    limit: usize,
) -> Result<Vec<MemoryRecord>, AppError> {
    let batches = table
        .query()
        .only_if(filter)
        .select(Select::columns(&[
            "id",
            "actor_id",
            "project_id",
            "thread_id",
            "execution_run_id",
            "owner_agent_id",
            "recorded_by_agent_id",
            "memory_scope",
            "content",
            "embedding",
            "memory_category",
            "metadata",
            "created_at",
            "updated_at",
        ]))
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    let mut records = parse_memory_records(&batches, deployment_id)?;
    records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    records.truncate(limit);
    Ok(records)
}

pub async fn search_memories_in_table(
    table: &Table,
    deployment_id: i64,
    query_embedding: &[f32],
    filters: &MemoryQueryFilters,
    limit: usize,
) -> Result<Vec<MemoryRecord>, AppError> {
    let filter = build_semantic_memory_filter(filters);
    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .distance_type(DistanceType::Cosine)
        .only_if(filter)
        .limit(limit)
        .select(Select::columns(&[
            "id",
            "actor_id",
            "project_id",
            "thread_id",
            "execution_run_id",
            "owner_agent_id",
            "recorded_by_agent_id",
            "memory_scope",
            "content",
            "embedding",
            "memory_category",
            "metadata",
            "created_at",
            "updated_at",
            "_distance",
        ]))
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    parse_memory_records(&batches, deployment_id)
}

pub async fn search_memories_full_text_in_table(
    table: &Table,
    deployment_id: i64,
    query: &str,
    filters: &MemoryQueryFilters,
    limit: usize,
) -> Result<Vec<MemoryRecord>, AppError> {
    let filter = build_semantic_memory_filter(filters);
    let batches = table
        .query()
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(filter)
        .limit(limit)
        .select(Select::columns(&[
            "id",
            "actor_id",
            "project_id",
            "thread_id",
            "execution_run_id",
            "owner_agent_id",
            "recorded_by_agent_id",
            "memory_scope",
            "content",
            "embedding",
            "memory_category",
            "metadata",
            "created_at",
            "updated_at",
        ]))
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    let mut records = parse_memory_records(&batches, deployment_id)?;
    records.truncate(limit);
    Ok(records)
}

fn build_semantic_memory_filter(filters: &MemoryQueryFilters) -> String {
    let mut scope_filters = Vec::new();

    if let Some(actor_id) = filters.actor_id {
        scope_filters.push(format!(
            "(actor_id = {} AND memory_scope = '{}')",
            actor_id,
            memory::scope::ACTOR
        ));
    }
    if let Some(project_id) = filters.project_id {
        scope_filters.push(format!(
            "(project_id = {} AND memory_scope = '{}')",
            project_id,
            memory::scope::PROJECT
        ));
    }
    if let Some(thread_id) = filters.thread_id {
        scope_filters.push(format!(
            "(thread_id = {} AND memory_scope = '{}')",
            thread_id,
            memory::scope::THREAD
        ));
    }
    if let Some(agent_id) = filters.agent_id {
        scope_filters.push(format!(
            "(owner_agent_id = {} AND memory_scope = '{}')",
            agent_id,
            memory::scope::AGENT
        ));
    }

    let scope_expr = if scope_filters.is_empty() {
        "false".to_string()
    } else {
        scope_filters.join(" OR ")
    };

    let categories_expr = filters
        .categories
        .as_ref()
        .filter(|cats| !cats.is_empty())
        .map(|cats| {
            let joined = cats
                .iter()
                .map(|cat| format!("'{}'", cat.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(", ");
            format!(" AND memory_category IN ({})", joined)
        })
        .unwrap_or_default();

    format!(
        "id > 0 AND embedding IS NOT NULL AND ({}){}",
        scope_expr, categories_expr
    )
}

pub async fn ensure_memory_indices(table: &Table) -> Result<(), AppError> {
    let existing_columns: HashSet<Vec<String>> =
        crate::vector_store::existing_index_columns(table).await?;

    if !existing_columns.contains(&vec!["content".to_string()]) {
        table
            .create_index(&["content"], Index::FTS(Default::default()))
            .execute()
            .await
            .map_err(map_vector_store_error)?;
    }

    for column in [
        "memory_scope",
        "actor_id",
        "project_id",
        "thread_id",
        "owner_agent_id",
        "memory_category",
    ] {
        if !existing_columns.contains(&vec![column.to_string()]) {
            table
                .create_index(&[column], Index::BTree(BTreeIndexBuilder::default()))
                .execute()
                .await
                .map_err(map_vector_store_error)?;
        }
    }

    Ok(())
}

fn memory_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("id", DataType::Int64, false),
        Field::new("actor_id", DataType::Int64, true),
        Field::new("project_id", DataType::Int64, true),
        Field::new("thread_id", DataType::Int64, true),
        Field::new("execution_run_id", DataType::Int64, true),
        Field::new("owner_agent_id", DataType::Int64, true),
        Field::new("recorded_by_agent_id", DataType::Int64, true),
        Field::new("memory_scope", DataType::Utf8, false),
        Field::new("content", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                MEMORY_EMBEDDING_DIMENSION,
            ),
            true,
        ),
        Field::new("memory_category", DataType::Utf8, false),
        Field::new("metadata", DataType::Utf8, false),
        Field::new(
            "created_at",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
        Field::new(
            "updated_at",
            DataType::Timestamp(TimeUnit::Microsecond, Some("UTC".into())),
            false,
        ),
    ]))
}

fn build_memory_record_batch(records: &[MemoryRecord]) -> Result<RecordBatch, AppError> {
    let schema = memory_schema();

    let ids = Arc::new(Int64Array::from(
        records.iter().map(|r| r.id).collect::<Vec<_>>(),
    )) as ArrayRef;
    let actor_ids = Arc::new(Int64Array::from(
        records.iter().map(|r| r.actor_id).collect::<Vec<_>>(),
    )) as ArrayRef;
    let project_ids = Arc::new(Int64Array::from(
        records.iter().map(|r| r.project_id).collect::<Vec<_>>(),
    )) as ArrayRef;
    let thread_ids = Arc::new(Int64Array::from(
        records.iter().map(|r| r.thread_id).collect::<Vec<_>>(),
    )) as ArrayRef;
    let execution_run_ids = Arc::new(Int64Array::from(
        records
            .iter()
            .map(|r| r.execution_run_id)
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let owner_agent_ids = Arc::new(Int64Array::from(
        records.iter().map(|r| r.owner_agent_id).collect::<Vec<_>>(),
    )) as ArrayRef;
    let recorded_by_agent_ids = Arc::new(Int64Array::from(
        records
            .iter()
            .map(|r| r.recorded_by_agent_id)
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let memory_scopes = Arc::new(StringArray::from(
        records
            .iter()
            .map(|r| r.memory_scope.clone())
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let contents = Arc::new(StringArray::from(
        records
            .iter()
            .map(|r| r.content.clone())
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let embedding_values_vec = records
        .iter()
        .flat_map(|r| {
            r.embedding
                .as_ref()
                .into_iter()
                .flat_map(|embedding| embedding.iter().copied())
        })
        .collect::<Vec<_>>();
    let embedding_values = Arc::new(Float32Array::from(embedding_values_vec)) as ArrayRef;
    let embedding_nulls = records
        .iter()
        .map(|r| r.embedding.is_none())
        .collect::<Vec<_>>();
    let embeddings = Arc::new(
        FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            MEMORY_EMBEDDING_DIMENSION,
            embedding_values,
            Some(embedding_nulls.into()),
        )
        .map_err(|err| {
            AppError::Internal(format!("Failed to build memory embedding array: {}", err))
        })?,
    ) as ArrayRef;
    let categories = Arc::new(StringArray::from(
        records
            .iter()
            .map(|r| r.memory_category.clone())
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let metadata = Arc::new(StringArray::from(
        records
            .iter()
            .map(|r| r.metadata.to_string())
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let created_at = Arc::new(
        TimestampMicrosecondArray::from(
            records
                .iter()
                .map(|r| r.created_at.timestamp_micros())
                .collect::<Vec<_>>(),
        )
        .with_timezone("UTC"),
    ) as ArrayRef;
    let updated_at = Arc::new(
        TimestampMicrosecondArray::from(
            records
                .iter()
                .map(|r| r.updated_at.timestamp_micros())
                .collect::<Vec<_>>(),
        )
        .with_timezone("UTC"),
    ) as ArrayRef;

    RecordBatch::try_new(
        schema,
        vec![
            ids,
            actor_ids,
            project_ids,
            thread_ids,
            execution_run_ids,
            owner_agent_ids,
            recorded_by_agent_ids,
            memory_scopes,
            contents,
            embeddings,
            categories,
            metadata,
            created_at,
            updated_at,
        ],
    )
    .map_err(|err| AppError::Internal(format!("Failed to build memory record batch: {}", err)))
}

fn parse_memory_records(
    batches: &[RecordBatch],
    deployment_id: i64,
) -> Result<Vec<MemoryRecord>, AppError> {
    let mut records = Vec::new();

    for batch in batches {
        let ids = batch
            .column_by_name("id")
            .ok_or_else(|| AppError::Internal("Missing memory id column".to_string()))?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| AppError::Internal("Invalid memory id column type".to_string()))?;
        let actor_ids = batch
            .column_by_name("actor_id")
            .ok_or_else(|| AppError::Internal("Missing memory actor_id column".to_string()))?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| AppError::Internal("Invalid memory actor_id column type".to_string()))?;
        let project_ids = batch
            .column_by_name("project_id")
            .ok_or_else(|| AppError::Internal("Missing memory project_id column".to_string()))?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory project_id column type".to_string())
            })?;
        let thread_ids = batch
            .column_by_name("thread_id")
            .ok_or_else(|| AppError::Internal("Missing memory thread_id column".to_string()))?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory thread_id column type".to_string())
            })?;
        let execution_run_ids = batch
            .column_by_name("execution_run_id")
            .ok_or_else(|| {
                AppError::Internal("Missing memory execution_run_id column".to_string())
            })?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory execution_run_id column type".to_string())
            })?;
        let owner_agent_ids = batch
            .column_by_name("owner_agent_id")
            .ok_or_else(|| AppError::Internal("Missing memory owner_agent_id column".to_string()))?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory owner_agent_id column type".to_string())
            })?;
        let recorded_by_agent_ids = batch
            .column_by_name("recorded_by_agent_id")
            .ok_or_else(|| {
                AppError::Internal("Missing memory recorded_by_agent_id column".to_string())
            })?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory recorded_by_agent_id column type".to_string())
            })?;
        let memory_scopes = batch
            .column_by_name("memory_scope")
            .ok_or_else(|| AppError::Internal("Missing memory scope column".to_string()))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| AppError::Internal("Invalid memory scope column type".to_string()))?;
        let contents = batch
            .column_by_name("content")
            .ok_or_else(|| AppError::Internal("Missing memory content column".to_string()))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| AppError::Internal("Invalid memory content column type".to_string()))?;
        let embeddings = batch
            .column_by_name("embedding")
            .ok_or_else(|| AppError::Internal("Missing memory embedding column".to_string()))?
            .as_any()
            .downcast_ref::<FixedSizeListArray>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory embedding column type".to_string())
            })?;
        let categories = batch
            .column_by_name("memory_category")
            .ok_or_else(|| AppError::Internal("Missing memory category column".to_string()))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| AppError::Internal("Invalid memory category column type".to_string()))?;
        let metadata = batch
            .column_by_name("metadata")
            .ok_or_else(|| AppError::Internal("Missing memory metadata column".to_string()))?
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| AppError::Internal("Invalid memory metadata column type".to_string()))?;
        let created_at = batch
            .column_by_name("created_at")
            .ok_or_else(|| AppError::Internal("Missing memory created_at column".to_string()))?
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory created_at column type".to_string())
            })?;
        let updated_at = batch
            .column_by_name("updated_at")
            .ok_or_else(|| AppError::Internal("Missing memory updated_at column".to_string()))?
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory updated_at column type".to_string())
            })?;
        let embedding_values = embeddings
            .values()
            .as_any()
            .downcast_ref::<Float32Array>()
            .ok_or_else(|| {
                AppError::Internal("Invalid memory embedding values type".to_string())
            })?;

        for idx in 0..batch.num_rows() {
            let id = ids.value(idx);
            let embedding = if embeddings.is_null(idx) {
                None
            } else {
                Some(read_embedding_at_row(embedding_values, idx))
            };

            let metadata_value = serde_json::from_str::<serde_json::Value>(metadata.value(idx))
                .unwrap_or_else(|_| serde_json::json!({}));

            let created_at_ts = created_at.value(idx);
            let updated_at_ts = updated_at.value(idx);
            let Some(created_at) = DateTime::<Utc>::from_timestamp_micros(created_at_ts) else {
                continue;
            };
            let Some(updated_at) = DateTime::<Utc>::from_timestamp_micros(updated_at_ts) else {
                continue;
            };

            records.push(MemoryRecord {
                id,
                deployment_id,
                actor_id: (!actor_ids.is_null(idx)).then(|| actor_ids.value(idx)),
                project_id: (!project_ids.is_null(idx)).then(|| project_ids.value(idx)),
                thread_id: (!thread_ids.is_null(idx)).then(|| thread_ids.value(idx)),
                execution_run_id: (!execution_run_ids.is_null(idx))
                    .then(|| execution_run_ids.value(idx)),
                owner_agent_id: (!owner_agent_ids.is_null(idx)).then(|| owner_agent_ids.value(idx)),
                recorded_by_agent_id: (!recorded_by_agent_ids.is_null(idx))
                    .then(|| recorded_by_agent_ids.value(idx)),
                memory_scope: memory_scopes.value(idx).to_string(),
                content: contents.value(idx).to_string(),
                embedding,
                memory_category: categories.value(idx).to_string(),
                metadata: metadata_value,
                created_at,
                updated_at,
            });
        }
    }

    Ok(records)
}

fn read_embedding_at_row(embedding_values: &Float32Array, row_idx: usize) -> Vec<f32> {
    let start = row_idx * MEMORY_EMBEDDING_DIMENSION as usize;
    let end = start + MEMORY_EMBEDDING_DIMENSION as usize;
    (start..end)
        .map(|value_idx| embedding_values.value(value_idx))
        .collect::<Vec<_>>()
}
