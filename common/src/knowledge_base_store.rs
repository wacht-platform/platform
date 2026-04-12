use std::collections::HashSet;
use std::sync::Arc;

use arrow_array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, Int64Array, RecordBatch, StringArray,
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use futures::TryStreamExt;
use lancedb::index::Index;
use lancedb::index::scalar::BTreeIndexBuilder;
use lancedb::index::scalar::FullTextSearchQuery;
use lancedb::query::{ExecutableQuery, QueryBase, Select};
use lancedb::{Connection, DistanceType, Error as LanceError, Table};
use models::ai_knowledge_base::DocumentChunkSearchResult;
use models::error::AppError;
use models::hybrid_search::{FullTextSearchResult, HybridSearchKbResult};

use crate::vector_store::{VectorStoreConfig, map_vector_store_error};

const KB_CHUNKS_TABLE: &str = "knowledge_base_chunks";
const RELEVANCE_SCORE_COLUMN: &str = "_relevance_score";
const KNOWLEDGE_EMBEDDING_DIMENSION: i32 = 1536;

#[derive(Debug, Clone)]
pub struct KnowledgeBaseChunkRecord {
    pub knowledge_base_id: i64,
    pub document_id: i64,
    pub path: String,
    pub title: String,
    pub description: Option<String>,
    pub content: String,
    pub embedding: Option<Vec<f32>>,
}

pub async fn replace_document_chunks(
    config: &VectorStoreConfig,
    document_id: i64,
    chunks: &[KnowledgeBaseChunkRecord],
) -> Result<(), AppError> {
    let table = open_or_create_kb_table(config).await?;

    table
        .delete(&format!("document_id = {}", document_id))
        .await
        .map_err(map_vector_store_error)?;

    if chunks.is_empty() {
        return Ok(());
    }

    let batch = build_record_batch(chunks)?;
    table
        .add(batch)
        .execute()
        .await
        .map_err(map_vector_store_error)?;

    Ok(())
}

pub async fn ensure_knowledge_base_indices(config: &VectorStoreConfig) -> Result<(), AppError> {
    let table = open_or_create_kb_table(config).await?;
    ensure_kb_indices(&table).await
}

pub async fn count_indexable_knowledge_base_rows(
    config: &VectorStoreConfig,
) -> Result<usize, AppError> {
    crate::vector_store::count_rows_with_filter(
        config,
        KB_CHUNKS_TABLE,
        "knowledge_base_id > 0 AND embedding IS NOT NULL",
    )
    .await
}

pub async fn knowledge_base_vector_index_exists(
    config: &VectorStoreConfig,
) -> Result<bool, AppError> {
    crate::vector_store::vector_index_exists(config, KB_CHUNKS_TABLE, "embedding").await
}

pub async fn create_knowledge_base_vector_index(
    config: &VectorStoreConfig,
) -> Result<(), AppError> {
    crate::vector_store::create_auto_vector_index(config, KB_CHUNKS_TABLE, kb_schema(), "embedding")
        .await
}

pub async fn optimize_knowledge_base_vector_index(
    config: &VectorStoreConfig,
) -> Result<(), AppError> {
    crate::vector_store::optimize_vector_index(config, KB_CHUNKS_TABLE).await
}

pub async fn delete_document_chunks(
    config: &VectorStoreConfig,
    document_id: i64,
) -> Result<(), AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(()),
    };

    table
        .delete(&format!("document_id = {}", document_id))
        .await
        .map_err(map_vector_store_error)?;

    Ok(())
}

pub async fn delete_knowledge_base_chunks(
    config: &VectorStoreConfig,
    knowledge_base_id: i64,
) -> Result<(), AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(()),
    };

    table
        .delete(&format!("knowledge_base_id = {}", knowledge_base_id))
        .await
        .map_err(map_vector_store_error)?;

    Ok(())
}

pub async fn search_full_text(
    config: &VectorStoreConfig,
    knowledge_base_ids: &[i64],
    query: &str,
    limit: usize,
) -> Result<Vec<FullTextSearchResult>, AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(Vec::new()),
    };

    let batches = table
        .query()
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(knowledge_base_ids))
        .limit(limit)
        .select(Select::columns(&[
            "document_id",
            "knowledge_base_id",
            "content",
            "title",
            "description",
            "_score",
        ]))
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    Ok(parse_full_text_results(&batches))
}

pub async fn search_vector(
    config: &VectorStoreConfig,
    knowledge_base_ids: &[i64],
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<DocumentChunkSearchResult>, AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(Vec::new()),
    };

    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .distance_type(DistanceType::Cosine)
        .only_if(searchable_kb_filter(knowledge_base_ids))
        .limit(limit)
        .select(Select::columns(&[
            "document_id",
            "knowledge_base_id",
            "content",
            "title",
            "description",
            "_distance",
        ]))
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    Ok(parse_vector_results(&batches))
}

pub async fn search_hybrid(
    config: &VectorStoreConfig,
    knowledge_base_ids: &[i64],
    query: &str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<HybridSearchKbResult>, AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(Vec::new()),
    };

    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .distance_type(DistanceType::Cosine)
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(knowledge_base_ids))
        .limit(limit)
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    Ok(parse_hybrid_results(&batches))
}

pub async fn open_knowledge_base_table(
    config: &VectorStoreConfig,
) -> Result<Option<Table>, AppError> {
    open_kb_table(config).await
}

pub async fn open_knowledge_base_table_in_connection(
    conn: &Connection,
) -> Result<Option<Table>, AppError> {
    match conn.open_table(KB_CHUNKS_TABLE).execute().await {
        Ok(table) => Ok(Some(table)),
        Err(LanceError::TableNotFound { .. }) => Ok(None),
        Err(err) => Err(map_vector_store_error(err)),
    }
}

pub async fn search_full_text_in_table(
    table: &Table,
    knowledge_base_ids: &[i64],
    query: &str,
    limit: usize,
) -> Result<Vec<FullTextSearchResult>, AppError> {
    let batches = table
        .query()
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(knowledge_base_ids))
        .limit(limit)
        .select(Select::columns(&[
            "document_id",
            "knowledge_base_id",
            "content",
            "title",
            "description",
            "_score",
        ]))
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    Ok(parse_full_text_results(&batches))
}

pub async fn search_vector_in_table(
    table: &Table,
    knowledge_base_ids: &[i64],
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<DocumentChunkSearchResult>, AppError> {
    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .distance_type(DistanceType::Cosine)
        .only_if(searchable_kb_filter(knowledge_base_ids))
        .limit(limit)
        .select(Select::columns(&[
            "document_id",
            "knowledge_base_id",
            "content",
            "title",
            "description",
            "_distance",
        ]))
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    Ok(parse_vector_results(&batches))
}

pub async fn search_hybrid_in_table(
    table: &Table,
    knowledge_base_ids: &[i64],
    query: &str,
    query_embedding: &[f32],
    limit: usize,
) -> Result<Vec<HybridSearchKbResult>, AppError> {
    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .distance_type(DistanceType::Cosine)
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(knowledge_base_ids))
        .limit(limit)
        .execute()
        .await
        .map_err(map_vector_store_error)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(map_vector_store_error)?;

    Ok(parse_hybrid_results(&batches))
}

fn kb_filter(knowledge_base_ids: &[i64]) -> String {
    let ids = knowledge_base_ids
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    format!("knowledge_base_id IN ({})", ids)
}

fn searchable_kb_filter(knowledge_base_ids: &[i64]) -> String {
    format!(
        "{} AND embedding IS NOT NULL",
        kb_filter(knowledge_base_ids)
    )
}

async fn open_or_create_kb_table(config: &VectorStoreConfig) -> Result<Table, AppError> {
    crate::vector_store::open_or_create_table(config, KB_CHUNKS_TABLE, kb_schema()).await
}

async fn open_kb_table(config: &VectorStoreConfig) -> Result<Option<Table>, AppError> {
    crate::vector_store::open_table(config, KB_CHUNKS_TABLE).await
}

async fn ensure_kb_indices(table: &Table) -> Result<(), AppError> {
    let existing_columns: HashSet<Vec<String>> =
        crate::vector_store::existing_index_columns(table).await?;

    if !existing_columns.contains(&vec!["content".to_string()]) {
        tracing::info!(
            table = KB_CHUNKS_TABLE,
            "Creating knowledge base full-text index"
        );
        table
            .create_index(&["content"], Index::FTS(Default::default()))
            .execute()
            .await
            .map_err(map_vector_store_error)?;
    }

    if !existing_columns.contains(&vec!["knowledge_base_id".to_string()]) {
        tracing::info!(
            table = KB_CHUNKS_TABLE,
            "Creating knowledge base knowledge_base_id btree index"
        );
        table
            .create_index(
                &["knowledge_base_id"],
                Index::BTree(BTreeIndexBuilder::default()),
            )
            .execute()
            .await
            .map_err(map_vector_store_error)?;
    }

    if !existing_columns.contains(&vec!["document_id".to_string()]) {
        tracing::info!(
            table = KB_CHUNKS_TABLE,
            "Creating knowledge base document_id btree index"
        );
        table
            .create_index(&["document_id"], Index::BTree(BTreeIndexBuilder::default()))
            .execute()
            .await
            .map_err(map_vector_store_error)?;
    }

    Ok(())
}

fn kb_schema() -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new("knowledge_base_id", DataType::Int64, false),
        Field::new("document_id", DataType::Int64, false),
        Field::new("path", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("description", DataType::Utf8, true),
        Field::new("content", DataType::Utf8, false),
        Field::new(
            "embedding",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                KNOWLEDGE_EMBEDDING_DIMENSION,
            ),
            true,
        ),
    ]))
}

fn build_record_batch(chunks: &[KnowledgeBaseChunkRecord]) -> Result<RecordBatch, AppError> {
    let schema = kb_schema();
    let knowledge_base_ids = Arc::new(Int64Array::from(
        chunks
            .iter()
            .map(|chunk| chunk.knowledge_base_id)
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let document_ids = Arc::new(Int64Array::from(
        chunks
            .iter()
            .map(|chunk| chunk.document_id)
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let paths = Arc::new(StringArray::from(
        chunks
            .iter()
            .map(|chunk| chunk.path.as_str())
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let titles = Arc::new(StringArray::from(
        chunks
            .iter()
            .map(|chunk| chunk.title.as_str())
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let descriptions = Arc::new(StringArray::from(
        chunks
            .iter()
            .map(|chunk| chunk.description.as_deref())
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let contents = Arc::new(StringArray::from(
        chunks
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>(),
    )) as ArrayRef;

    let embedding_values = Arc::new(Float32Array::from(
        chunks
            .iter()
            .flat_map(|chunk| {
                chunk
                    .embedding
                    .as_ref()
                    .into_iter()
                    .flat_map(|embedding| embedding.iter().copied())
            })
            .collect::<Vec<_>>(),
    )) as ArrayRef;
    let embedding_nulls = chunks
        .iter()
        .map(|chunk| chunk.embedding.is_none())
        .collect::<Vec<_>>();
    let embeddings = Arc::new(
        FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            KNOWLEDGE_EMBEDDING_DIMENSION,
            embedding_values,
            Some(embedding_nulls.into()),
        )
        .map_err(|err| AppError::Internal(format!("Failed to build embedding array: {}", err)))?,
    ) as ArrayRef;

    RecordBatch::try_new(
        schema,
        vec![
            knowledge_base_ids,
            document_ids,
            paths,
            titles,
            descriptions,
            contents,
            embeddings,
        ],
    )
    .map_err(|err| AppError::Internal(format!("Failed to build LanceDB record batch: {}", err)))
}

fn parse_vector_results(batches: &[RecordBatch]) -> Vec<DocumentChunkSearchResult> {
    let mut results = Vec::new();

    for batch in batches {
        let document_ids = int64_column(batch, "document_id");
        let knowledge_base_ids = int64_column(batch, "knowledge_base_id");
        let contents = string_column(batch, "content");
        let titles = string_column(batch, "title");
        let descriptions = string_column(batch, "description");
        let distances = float32_column(batch, "_distance");

        for row in 0..batch.num_rows() {
            results.push(DocumentChunkSearchResult {
                document_id: document_ids.value(row),
                knowledge_base_id: knowledge_base_ids.value(row),
                content: contents.value(row).to_string(),
                score: distances.value(row) as f64,
                chunk_index: 0,
                document_title: string_value(titles, row),
                document_description: string_value(descriptions, row),
            });
        }
    }

    results
}

fn parse_full_text_results(batches: &[RecordBatch]) -> Vec<FullTextSearchResult> {
    let mut results = Vec::new();

    for batch in batches {
        let document_ids = int64_column(batch, "document_id");
        let knowledge_base_ids = int64_column(batch, "knowledge_base_id");
        let contents = string_column(batch, "content");
        let titles = string_column(batch, "title");
        let descriptions = string_column(batch, "description");
        let scores = float32_column(batch, "_score");

        for row in 0..batch.num_rows() {
            results.push(FullTextSearchResult {
                document_id: document_ids.value(row),
                knowledge_base_id: knowledge_base_ids.value(row),
                chunk_index: 0,
                content: contents.value(row).to_string(),
                text_rank: scores.value(row) as f64,
                document_title: string_value(titles, row),
                document_description: string_value(descriptions, row),
            });
        }
    }

    results
}

fn parse_hybrid_results(batches: &[RecordBatch]) -> Vec<HybridSearchKbResult> {
    let mut results = Vec::new();

    for batch in batches {
        let document_ids = int64_column(batch, "document_id");
        let knowledge_base_ids = int64_column(batch, "knowledge_base_id");
        let contents = string_column(batch, "content");
        let titles = string_column(batch, "title");
        let descriptions = string_column(batch, "description");
        let scores = float32_column(batch, RELEVANCE_SCORE_COLUMN);
        let vector_distances = optional_float32_column(batch, "_distance");
        let text_scores = optional_float32_column(batch, "_score");

        for row in 0..batch.num_rows() {
            results.push(HybridSearchKbResult {
                document_id: document_ids.value(row),
                knowledge_base_id: knowledge_base_ids.value(row),
                chunk_index: 0,
                content: contents.value(row).to_string(),
                document_title: string_value(titles, row),
                document_description: string_value(descriptions, row),
                vector_similarity: vector_distances
                    .map(|array| array.value(row) as f64)
                    .unwrap_or_default(),
                text_rank: text_scores
                    .map(|array| array.value(row) as f64)
                    .unwrap_or_default(),
                combined_score: scores.value(row) as f64,
            });
        }
    }

    results
}

fn int64_column<'a>(batch: &'a RecordBatch, name: &str) -> &'a Int64Array {
    batch
        .column_by_name(name)
        .unwrap()
        .as_any()
        .downcast_ref()
        .unwrap()
}

fn string_column<'a>(batch: &'a RecordBatch, name: &str) -> &'a StringArray {
    batch
        .column_by_name(name)
        .unwrap()
        .as_any()
        .downcast_ref()
        .unwrap()
}

fn float32_column<'a>(batch: &'a RecordBatch, name: &str) -> &'a Float32Array {
    batch
        .column_by_name(name)
        .unwrap()
        .as_any()
        .downcast_ref()
        .unwrap()
}

fn optional_float32_column<'a>(batch: &'a RecordBatch, name: &str) -> Option<&'a Float32Array> {
    batch
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref())
}

fn string_value(array: &StringArray, row: usize) -> Option<String> {
    if array.is_null(row) {
        None
    } else {
        Some(array.value(row).to_string())
    }
}
