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
use lancedb::table::NewColumnTransform;
use lancedb::{Connection, DistanceType, Error as LanceError, Table};
use models::ai_knowledge_base::DocumentChunkSearchResult;
use models::error::AppError;
use models::hybrid_search::{FullTextSearchResult, HybridSearchKbResult};

use crate::vector_store::{VectorStoreConfig, map_vector_store_error};

const KB_CHUNKS_TABLE: &str = "knowledge_base_chunks";
const RELEVANCE_SCORE_COLUMN: &str = "_relevance_score";
const KNOWLEDGE_EMBEDDING_DIMENSION: i32 = 1536;
const KNOWLEDGE_EMBEDDING_DIMENSION_768: i32 = 768;

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
    embedding_dimension: i32,
) -> Result<(), AppError> {
    let table = open_or_create_kb_table(config).await?;

    table
        .delete(&format!("document_id = {}", document_id))
        .await
        .map_err(map_vector_store_error)?;

    if chunks.is_empty() {
        return Ok(());
    }

    let batch = build_record_batch(chunks, embedding_dimension)?;
    table
        .add(batch)
        .execute()
        .await
        .map_err(map_vector_store_error)?;

    Ok(())
}

pub async fn ensure_knowledge_base_indices(config: &VectorStoreConfig) -> Result<(), AppError> {
    let table =
        crate::vector_store::open_or_create_table(config, KB_CHUNKS_TABLE, kb_schema()).await?;
    ensure_kb_table_schema(&table).await?;
    ensure_kb_indices(&table).await
}

pub async fn count_indexable_knowledge_base_rows(
    config: &VectorStoreConfig,
    embedding_dimension: i32,
) -> Result<usize, AppError> {
    let table = open_or_create_kb_table(config).await?;
    table
        .count_rows(Some(format!(
            "knowledge_base_id > 0 AND {} IS NOT NULL",
            embedding_column_for_dimension(embedding_dimension)
        )))
        .await
        .map_err(map_vector_store_error)
}

pub async fn knowledge_base_vector_index_exists(
    config: &VectorStoreConfig,
    embedding_dimension: i32,
) -> Result<bool, AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(false),
    };

    let existing = table.list_indices().await.map_err(map_vector_store_error)?;
    Ok(existing.into_iter().any(|idx| {
        idx.columns == vec![embedding_column_for_dimension(embedding_dimension).to_string()]
    }))
}

pub async fn create_knowledge_base_vector_index(
    config: &VectorStoreConfig,
    embedding_dimension: i32,
) -> Result<(), AppError> {
    let table = open_or_create_kb_table(config).await?;
    table
        .create_index(
            &[embedding_column_for_dimension(embedding_dimension)],
            Index::Auto,
        )
        .execute()
        .await
        .map_err(map_vector_store_error)
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
    embedding_dimension: i32,
) -> Result<Vec<FullTextSearchResult>, AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(Vec::new()),
    };

    let batches = table
        .query()
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(
            knowledge_base_ids,
            embedding_dimension,
        ))
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
    embedding_dimension: i32,
) -> Result<Vec<DocumentChunkSearchResult>, AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(Vec::new()),
    };

    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .column(embedding_column_for_dimension(embedding_dimension))
        .distance_type(DistanceType::Cosine)
        .only_if(searchable_kb_filter(
            knowledge_base_ids,
            embedding_dimension,
        ))
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
    embedding_dimension: i32,
) -> Result<Vec<HybridSearchKbResult>, AppError> {
    let table = match open_kb_table(config).await? {
        Some(table) => table,
        None => return Ok(Vec::new()),
    };

    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .column(embedding_column_for_dimension(embedding_dimension))
        .distance_type(DistanceType::Cosine)
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(
            knowledge_base_ids,
            embedding_dimension,
        ))
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
    embedding_dimension: i32,
) -> Result<Vec<FullTextSearchResult>, AppError> {
    let batches = table
        .query()
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(
            knowledge_base_ids,
            embedding_dimension,
        ))
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
    embedding_dimension: i32,
) -> Result<Vec<DocumentChunkSearchResult>, AppError> {
    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .column(embedding_column_for_dimension(embedding_dimension))
        .distance_type(DistanceType::Cosine)
        .only_if(searchable_kb_filter(
            knowledge_base_ids,
            embedding_dimension,
        ))
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
    embedding_dimension: i32,
) -> Result<Vec<HybridSearchKbResult>, AppError> {
    let batches = table
        .vector_search(query_embedding.to_vec())
        .map_err(map_vector_store_error)?
        .column(embedding_column_for_dimension(embedding_dimension))
        .distance_type(DistanceType::Cosine)
        .full_text_search(FullTextSearchQuery::new(query.to_string()))
        .only_if(searchable_kb_filter(
            knowledge_base_ids,
            embedding_dimension,
        ))
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

fn searchable_kb_filter(knowledge_base_ids: &[i64], embedding_dimension: i32) -> String {
    format!(
        "{} AND {} IS NOT NULL",
        kb_filter(knowledge_base_ids),
        embedding_column_for_dimension(embedding_dimension),
    )
}

fn embedding_column_for_dimension(embedding_dimension: i32) -> &'static str {
    match embedding_dimension {
        768 => "embedding_768",
        _ => "embedding",
    }
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

async fn ensure_kb_table_schema(table: &Table) -> Result<(), AppError> {
    let schema = table.schema().await.map_err(map_vector_store_error)?;

    if schema.field_with_name("embedding_768").is_err() {
        tracing::info!(
            table = KB_CHUNKS_TABLE,
            "Adding missing embedding_768 column to knowledge base table"
        );
        table
            .add_columns(
                NewColumnTransform::AllNulls(Arc::new(Schema::new(vec![Field::new(
                    "embedding_768",
                    DataType::FixedSizeList(
                        Arc::new(Field::new("item", DataType::Float32, true)),
                        KNOWLEDGE_EMBEDDING_DIMENSION_768,
                    ),
                    true,
                )]))),
                None,
            )
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
        Field::new(
            "embedding_768",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                KNOWLEDGE_EMBEDDING_DIMENSION_768,
            ),
            true,
        ),
    ]))
}

fn build_record_batch(
    chunks: &[KnowledgeBaseChunkRecord],
    embedding_dimension: i32,
) -> Result<RecordBatch, AppError> {
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

    let embeddings =
        build_embedding_array(chunks, embedding_dimension, KNOWLEDGE_EMBEDDING_DIMENSION)?;
    let embeddings_768 = build_embedding_array(
        chunks,
        embedding_dimension,
        KNOWLEDGE_EMBEDDING_DIMENSION_768,
    )?;

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
            embeddings_768,
        ],
    )
    .map_err(|err| AppError::Internal(format!("Failed to build LanceDB record batch: {}", err)))
}

fn build_embedding_array(
    chunks: &[KnowledgeBaseChunkRecord],
    source_dimension: i32,
    target_dimension: i32,
) -> Result<ArrayRef, AppError> {
    let mut values = Vec::with_capacity(chunks.len() * target_dimension as usize);
    let mut nulls = Vec::with_capacity(chunks.len());

    // Arrow NullBuffer convention: `true` = valid, `false` = null.
    for chunk in chunks {
        let use_embedding = source_dimension == target_dimension && chunk.embedding.is_some();
        if use_embedding {
            let embedding = chunk.embedding.as_ref().expect("checked is_some");
            if embedding.len() != target_dimension as usize {
                return Err(AppError::Validation(format!(
                    "Embedding length {} does not match target dimension {}",
                    embedding.len(),
                    target_dimension
                )));
            }
            values.extend_from_slice(embedding);
            nulls.push(true);
        } else {
            values.extend(std::iter::repeat(0.0f32).take(target_dimension as usize));
            nulls.push(false);
        }
    }

    let values = Arc::new(Float32Array::from(values)) as ArrayRef;
    let array = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        target_dimension,
        values,
        Some(nulls.into()),
    )
    .map_err(|err| AppError::Internal(format!("Failed to build embedding array: {}", err)))?;
    Ok(Arc::new(array) as ArrayRef)
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
