use async_trait::async_trait;
use commands::ResolveDeploymentStorageCommand;
use common::error::AppError;
use common::state::AppState;
use common::{
    connect_vector_store, open_knowledge_base_table_in_connection, open_memory_table_in_connection,
    search_full_text_in_table, search_hybrid_in_table, search_vector_in_table,
};
use lancedb::{Connection, Table};
use models::ai_knowledge_base::DocumentChunkSearchResult;
use models::hybrid_search::{FullTextSearchResult, HybridSearchKbResult};
use models::MemoryRecord;
use std::sync::Arc;
use tokio::sync::RwLock;

#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn search_kb_full_text(
        &self,
        kb_ids: &[i64],
        query: &str,
        limit: usize,
    ) -> Result<Vec<FullTextSearchResult>, AppError>;

    async fn search_kb_vector(
        &self,
        kb_ids: &[i64],
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<DocumentChunkSearchResult>, AppError>;

    async fn search_kb_hybrid(
        &self,
        kb_ids: &[i64],
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<HybridSearchKbResult>, AppError>;

    async fn get_startup_memories(
        &self,
        thread_id: i64,
        actor_id: i64,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, AppError>;
}

pub trait VectorStoreFactory: Send + Sync {
    fn create(&self, deployment_id: i64, embedding_dimension: i32) -> Arc<dyn VectorStore>;
}

pub struct LanceDbVectorStore {
    app_state: AppState,
    deployment_id: i64,
    embedding_dimension: i32,
    cached_connection: RwLock<Option<Connection>>,
    cached_kb_table: RwLock<Option<Table>>,
    cached_memory_table: RwLock<Option<Table>>,
}

impl LanceDbVectorStore {
    pub fn new(app_state: AppState, deployment_id: i64, embedding_dimension: i32) -> Self {
        Self {
            app_state,
            deployment_id,
            embedding_dimension,
            cached_connection: RwLock::new(None),
            cached_kb_table: RwLock::new(None),
            cached_memory_table: RwLock::new(None),
        }
    }

    async fn get_connection(&self) -> Result<Connection, AppError> {
        {
            let cache = self.cached_connection.read().await;
            if let Some(conn) = cache.as_ref() {
                return Ok(conn.clone());
            }
        }

        let mut cache = self.cached_connection.write().await;
        if let Some(conn) = cache.as_ref() {
            return Ok(conn.clone());
        }

        let storage = ResolveDeploymentStorageCommand::new(self.deployment_id)
            .execute_with_deps(&common::deps::from_app(&self.app_state).db().enc())
            .await?;
        let config = storage.vector_store_config();
        let conn = connect_vector_store(&config).await?;
        *cache = Some(conn.clone());
        Ok(conn)
    }

    async fn get_kb_table(&self) -> Result<Option<Table>, AppError> {
        {
            let cache = self.cached_kb_table.read().await;
            if let Some(table) = cache.as_ref() {
                return Ok(Some(table.clone()));
            }
        }

        let mut cache = self.cached_kb_table.write().await;
        if let Some(table) = cache.as_ref() {
            return Ok(Some(table.clone()));
        }

        let conn = self.get_connection().await?;
        let table = open_knowledge_base_table_in_connection(&conn).await?;
        if let Some(table) = table.as_ref() {
            *cache = Some(table.clone());
        }
        Ok(table)
    }

    async fn get_memory_table(&self) -> Result<Option<Table>, AppError> {
        {
            let cache = self.cached_memory_table.read().await;
            if let Some(table) = cache.as_ref() {
                return Ok(Some(table.clone()));
            }
        }

        let mut cache = self.cached_memory_table.write().await;
        if let Some(table) = cache.as_ref() {
            return Ok(Some(table.clone()));
        }

        let conn = self.get_connection().await?;
        let table = open_memory_table_in_connection(&conn).await?;
        if let Some(table) = table.as_ref() {
            *cache = Some(table.clone());
        }
        Ok(table)
    }
}

#[async_trait]
impl VectorStore for LanceDbVectorStore {
    async fn search_kb_full_text(
        &self,
        kb_ids: &[i64],
        query: &str,
        limit: usize,
    ) -> Result<Vec<FullTextSearchResult>, AppError> {
        let Some(table) = self.get_kb_table().await? else {
            return Ok(Vec::new());
        };
        search_full_text_in_table(&table, kb_ids, query, limit, self.embedding_dimension).await
    }

    async fn search_kb_vector(
        &self,
        kb_ids: &[i64],
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<DocumentChunkSearchResult>, AppError> {
        let Some(table) = self.get_kb_table().await? else {
            return Ok(Vec::new());
        };
        search_vector_in_table(
            &table,
            kb_ids,
            query_embedding,
            limit,
            self.embedding_dimension,
        )
        .await
    }

    async fn search_kb_hybrid(
        &self,
        kb_ids: &[i64],
        query: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<HybridSearchKbResult>, AppError> {
        let Some(table) = self.get_kb_table().await? else {
            return Ok(Vec::new());
        };
        search_hybrid_in_table(
            &table,
            kb_ids,
            query,
            query_embedding,
            limit,
            self.embedding_dimension,
        )
        .await
    }

    async fn get_startup_memories(
        &self,
        thread_id: i64,
        actor_id: i64,
        limit: usize,
    ) -> Result<Vec<MemoryRecord>, AppError> {
        let Some(table) = self.get_memory_table().await? else {
            return Ok(Vec::new());
        };
        common::get_startup_memories_in_table(
            &table,
            self.deployment_id,
            thread_id,
            actor_id,
            limit,
            self.embedding_dimension,
        )
        .await
    }
}

pub struct LanceDbVectorStoreFactory {
    app_state: AppState,
}

impl LanceDbVectorStoreFactory {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }
}

impl VectorStoreFactory for LanceDbVectorStoreFactory {
    fn create(&self, deployment_id: i64, embedding_dimension: i32) -> Arc<dyn VectorStore> {
        Arc::new(LanceDbVectorStore::new(
            self.app_state.clone(),
            deployment_id,
            embedding_dimension,
        ))
    }
}
