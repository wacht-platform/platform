use crate::error::AppError;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, CreateFieldIndexCollectionBuilder, DeletePointsBuilder,
    Distance, FieldType, Filter, HnswConfigDiff, PointStruct, SearchPointsBuilder,
    UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};
use serde_json::Value;
use std::collections::HashMap;

pub struct QdrantService;

#[derive(Debug, Clone)]
pub struct DocumentChunk {
    pub id: i64,
    pub content: String,
    pub metadata: HashMap<String, Value>,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: i64,
    pub content: String,
    pub score: f32,
    pub metadata: HashMap<String, Value>,
}

impl QdrantService {
    pub async fn connect() -> Result<Qdrant, AppError> {
        let qdrant_url =
            std::env::var("QDRANT_URL").unwrap_or_else(|_| "http://localhost:6334".to_string());

        let client = if let Ok(api_key) = std::env::var("QDRANT_API_KEY") {
            Qdrant::from_url(&qdrant_url).api_key(api_key).build()
        } else {
            Qdrant::from_url(&qdrant_url).build()
        };

        client.map_err(|e| AppError::Internal(format!("Failed to connect to Qdrant: {}", e)))
    }

    pub fn default_collection() -> String {
        std::env::var("QDRANT_DEFAULT_COLLECTION").unwrap_or_else(|_| "knowledge_base".to_string())
    }

    pub async fn initialize() -> Result<(), AppError> {
        let client = Self::connect().await?;
        let collection_name = Self::default_collection();

        let collections = client
            .list_collections()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to list collections: {}", e)))?;

        let collection_exists = collections
            .collections
            .iter()
            .any(|c| c.name == collection_name);

        if !collection_exists {
            println!("Creating Qdrant collection: {}", collection_name);

            client
                .create_collection(
                    CreateCollectionBuilder::new(&collection_name)
                        .vectors_config(VectorParamsBuilder::new(768, Distance::Cosine))
                        .hnsw_config(HnswConfigDiff {
                            payload_m: Some(16), 
                            m: Some(0),         
                            ..Default::default()
                        }),
                )
                .await
                .map_err(|e| AppError::Internal(format!("Failed to create collection: {}", e)))?;

            println!(
                "Successfully created Qdrant collection: {}",
                collection_name
            );
        } else {
            println!("Qdrant collection already exists: {}", collection_name);
        }

        println!("Creating/verifying integer index on knowledge_base_id field...");
        match client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                &collection_name,
                "knowledge_base_id",
                FieldType::Integer,
            ))
            .await
        {
            Ok(_) => {}
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("already exists")
                    || error_msg.contains("Index already exists")
                {
                    println!("✅ Integer index on knowledge_base_id already exists");
                } else {
                    return Err(AppError::Internal(format!(
                        "Failed to create knowledge_base_id index: {}",
                        e
                    )));
                }
            }
        }

        Ok(())
    }

    pub async fn ensure_default_collection() -> Result<(), AppError> {
        let client = Self::connect().await?;
        let collection_name = Self::default_collection();

        let collections = client
            .list_collections()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to list collections: {}", e)))?;

        let collection_exists = collections
            .collections
            .iter()
            .any(|c| c.name == collection_name);

        if !collection_exists {
            return Self::initialize().await;
        }

        Ok(())
    }

    pub async fn upsert_documents(
        chunks: Vec<DocumentChunk>,
        knowledge_base_id: i64,
    ) -> Result<(), AppError> {
        if chunks.is_empty() {
            return Ok(());
        }

        Self::ensure_default_collection().await?;

        let collection_name = Self::default_collection();

        let points: Vec<PointStruct> = chunks
            .into_iter()
            .map(|chunk| {
                let mut payload_map = serde_json::Map::new();
                payload_map.insert(
                    "content".to_string(),
                    serde_json::Value::String(chunk.content),
                );

                payload_map.insert(
                    "knowledge_base_id".to_string(),
                    serde_json::Value::Number(knowledge_base_id.into()),
                );

                for (key, value) in chunk.metadata {
                    payload_map.insert(key, value);
                }

                let payload: Payload = serde_json::Value::Object(payload_map).try_into().unwrap();

                PointStruct::new(chunk.id as u64, chunk.embedding, payload)
            })
            .collect();

        let client = Self::connect().await?;
        let points_count = points.len();
        client
            .upsert_points(UpsertPointsBuilder::new(&collection_name, points))
            .await
            .map_err(|e| AppError::Internal(format!("Failed to upsert points: {}", e)))?;

        println!(
            "Upserted {} document chunks for knowledge base {} to Qdrant collection: {}",
            points_count, knowledge_base_id, collection_name
        );
        Ok(())
    }

    pub async fn search_similar(
        query_embedding: Vec<f32>,
        limit: u64,
        knowledge_base_id: i64,
    ) -> Result<Vec<SearchResult>, AppError> {
        let collection_name = Self::default_collection();

        let mut search_builder =
            SearchPointsBuilder::new(&collection_name, query_embedding, limit).with_payload(true);

        let conditions = vec![Condition::matches("knowledge_base_id", knowledge_base_id)];

        let filter = Filter::all(conditions);
        search_builder = search_builder.filter(filter);

        let client = Self::connect().await?;
        let search_result = client
            .search_points(search_builder)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to search points: {}", e)))?;

        let results: Vec<SearchResult> = search_result
            .result
            .into_iter()
            .map(|scored_point| {
                let id = scored_point.id
                    .map(|point_id| {
                        match point_id {
                            qdrant_client::qdrant::PointId { point_id_options: Some(qdrant_client::qdrant::point_id::PointIdOptions::Num(num)) } => num as i64,
                            _ => 0,
                        }
                    })
                    .unwrap_or_else(|| 0);

                let content = scored_point
                    .payload
                    .get("content")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "".to_string());

                let mut metadata = HashMap::new();
                for (key, value) in scored_point.payload {
                    if key != "content" {
                        let json_value = value.into_json();
                        metadata.insert(key, json_value);
                    }
                }

                SearchResult {
                    id,
                    content,
                    score: scored_point.score,
                    metadata,
                }
            })
            .collect();

        println!(
            "Found {} similar documents for knowledge base {} in collection: {}",
            results.len(),
            knowledge_base_id,
            collection_name
        );
        Ok(results)
    }

    pub async fn delete_by_metadata(
        knowledge_base_id: i64,
        additional_filters: HashMap<String, Value>,
    ) -> Result<(), AppError> {
        let collection_name = Self::default_collection();

        let mut conditions = vec![Condition::matches("knowledge_base_id", knowledge_base_id)];

        for (key, value) in additional_filters {
            let condition = match value {
                Value::String(s) => Condition::matches(key, s),
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Condition::matches(key, i)
                    } else {
                        continue;
                    }
                }
                Value::Bool(b) => Condition::matches(key, b),
                _ => continue,
            };
            conditions.push(condition);
        }

        let filter = Filter::all(conditions);

        let client = Self::connect().await?;
        client
            .delete_points(DeletePointsBuilder::new(&collection_name).points(filter))
            .await
            .map_err(|e| AppError::Internal(format!("Failed to delete points: {}", e)))?;

        println!(
            "Deleted points from knowledge base {} in collection '{}'",
            knowledge_base_id, collection_name
        );
        Ok(())
    }

    pub async fn delete_knowledge_base(knowledge_base_id: i64) -> Result<(), AppError> {
        Self::delete_by_metadata(knowledge_base_id, HashMap::new()).await?;

        println!(
            "Deleted all documents for knowledge base {}",
            knowledge_base_id
        );
        Ok(())
    }
}
