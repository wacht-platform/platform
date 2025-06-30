use crate::{error::AppError, state::AppState};
use llm::builder::{LLMBackend, LLMBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::Command;

#[derive(Debug, Clone)]
pub struct GenerateEmbeddingCommand {
    pub text: String,
}

impl GenerateEmbeddingCommand {
    pub fn new(text: String) -> Self {
        Self { text }
    }
}

impl Command for GenerateEmbeddingCommand {
    type Output = Vec<f32>;

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| {
            AppError::Internal("GEMINI_API_KEY environment variable not set".to_string())
        })?;

        let model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-004".to_string());

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model(&model)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to initialize Gemini LLM: {}", e)))?;

        let embeddings = llm
            .embed(vec![self.text])
            .await
            .map_err(|e| AppError::Internal(format!("Failed to generate embeddings: {}", e)))?;

        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("No embedding returned".to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct GenerateEmbeddingsCommand {
    pub texts: Vec<String>,
}

impl GenerateEmbeddingsCommand {
    pub fn new(texts: Vec<String>) -> Self {
        Self { texts }
    }
}

impl Command for GenerateEmbeddingsCommand {
    type Output = Vec<Vec<f32>>;

    async fn execute(self, _app_state: &AppState) -> Result<Self::Output, AppError> {
        if self.texts.is_empty() {
            return Ok(vec![]);
        }

        let api_key = std::env::var("GEMINI_API_KEY").map_err(|_| {
            AppError::Internal("GEMINI_API_KEY environment variable not set".to_string())
        })?;

        let model = std::env::var("GEMINI_EMBEDDING_MODEL")
            .unwrap_or_else(|_| "text-embedding-004".to_string());

        let llm = LLMBuilder::new()
            .backend(LLMBackend::Google)
            .api_key(&api_key)
            .model(&model)
            .build()
            .map_err(|e| AppError::Internal(format!("Failed to initialize Gemini LLM: {}", e)))?;

        let embeddings = llm
            .embed(self.texts)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to generate embeddings: {}", e)))?;

        Ok(embeddings)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentChunk {
    pub id: i64,
    pub content: String,
    pub metadata: HashMap<String, serde_json::Value>,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: i64,
    pub content: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct StoreKnowledgeBaseEmbeddingCommand {
    pub document_id: i64,
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub chunk_index: i32,
    pub content: String,
    pub embedding: Vec<f32>,
}

impl StoreKnowledgeBaseEmbeddingCommand {
    pub fn new(
        document_id: i64,
        deployment_id: i64,
        knowledge_base_id: i64,
        chunk_index: i32,
        content: String,
        embedding: Vec<f32>,
    ) -> Self {
        Self {
            document_id,
            deployment_id,
            knowledge_base_id,
            chunk_index,
            content,
            embedding,
        }
    }
}

impl Command for StoreKnowledgeBaseEmbeddingCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .store_knowledge_base_document(
                self.document_id,
                self.deployment_id,
                self.knowledge_base_id,
                self.chunk_index,
                &self.content,
                self.embedding,
            )
            .await
    }
}

#[derive(Debug, Clone)]
pub struct StoreMemoryEmbeddingCommand {
    pub memory_id: i64,
    pub deployment_id: i64,
    pub agent_id: i64,
    pub execution_context_id: i64,
    pub memory_type: String,
    pub content: String,
    pub embedding: Vec<f32>,
    pub importance: f32,
    pub access_count: i32,
}

impl StoreMemoryEmbeddingCommand {
    pub fn new(
        memory_id: i64,
        deployment_id: i64,
        agent_id: i64,
        execution_context_id: i64,
        memory_type: String,
        content: String,
        embedding: Vec<f32>,
        importance: f32,
        access_count: i32,
    ) -> Self {
        Self {
            memory_id,
            deployment_id,
            agent_id,
            execution_context_id,
            memory_type,
            content,
            embedding,
            importance,
            access_count,
        }
    }
}

impl Command for StoreMemoryEmbeddingCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .store_memory(
                self.memory_id,
                self.deployment_id,
                self.agent_id,
                self.execution_context_id,
                &self.memory_type,
                &self.content,
                self.embedding,
                self.importance,
                self.access_count,
            )
            .await
    }
}

/// Command to store conversation embeddings
#[derive(Debug, Clone)]
pub struct StoreConversationEmbeddingCommand {
    pub message_id: i64,
    pub deployment_id: i64,
    pub execution_context_id: i64,
    pub agent_id: i64,
    pub message_type: String,
    pub content: String,
    pub embedding: Vec<f32>,
}

impl StoreConversationEmbeddingCommand {
    pub fn new(
        message_id: i64,
        deployment_id: i64,
        execution_context_id: i64,
        agent_id: i64,
        message_type: String,
        content: String,
        embedding: Vec<f32>,
    ) -> Self {
        Self {
            message_id,
            deployment_id,
            execution_context_id,
            agent_id,
            message_type,
            content,
            embedding,
        }
    }
}

impl Command for StoreConversationEmbeddingCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .store_execution_message(
                self.message_id,
                self.deployment_id,
                self.execution_context_id,
                self.agent_id,
                &self.message_type,
                &self.content,
                self.embedding,
            )
            .await
    }
}

/// Command to store context embeddings
#[derive(Debug, Clone)]
pub struct StoreContextEmbeddingCommand {
    pub context_id: i64,
    pub deployment_id: i64,
    pub execution_context_id: i64,
    pub content: String,
    pub embedding: Vec<f32>,
    pub metadata: HashMap<String, serde_json::Value>,
}

impl StoreContextEmbeddingCommand {
    pub fn new(
        context_id: i64,
        deployment_id: i64,
        execution_context_id: i64,
        content: String,
        embedding: Vec<f32>,
        metadata: HashMap<String, serde_json::Value>,
    ) -> Self {
        Self {
            context_id,
            deployment_id,
            execution_context_id,
            content,
            embedding,
            metadata,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SearchKnowledgeBaseEmbeddingsCommand {
    pub knowledge_base_id: i64,
    pub query_embedding: Vec<f32>,
    pub limit: u64,
}

impl SearchKnowledgeBaseEmbeddingsCommand {
    pub fn new(knowledge_base_id: i64, query_embedding: Vec<f32>, limit: u64) -> Self {
        Self {
            knowledge_base_id,
            query_embedding,
            limit,
        }
    }
}

impl Command for SearchKnowledgeBaseEmbeddingsCommand {
    type Output = Vec<crate::services::clickhouse::DocumentSearchResult>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .search_knowledge_base_documents(
                self.knowledge_base_id,
                self.query_embedding,
                self.limit,
            )
            .await
    }
}

/// Command to search memory embeddings
#[derive(Debug, Clone)]
pub struct SearchMemoryEmbeddingsCommand {
    pub deployment_id: i64,
    pub agent_id: i64,
    pub query_embedding: Vec<f32>,
    pub limit: u64,
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

impl SearchMemoryEmbeddingsCommand {
    pub fn new(
        deployment_id: i64,
        agent_id: i64,
        query_embedding: Vec<f32>,
        limit: u64,
        filters: Option<HashMap<String, serde_json::Value>>,
    ) -> Self {
        Self {
            deployment_id,
            agent_id,
            query_embedding,
            limit,
            filters,
        }
    }
}

impl Command for SearchMemoryEmbeddingsCommand {
    type Output = Vec<SearchResult>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let memory_type_filter = self
            .filters
            .as_ref()
            .and_then(|f| f.get("memory_type"))
            .and_then(|v| v.as_str());

        let results = app_state
            .clickhouse_service
            .search_memories(
                self.agent_id,
                self.query_embedding,
                self.limit,
                memory_type_filter,
            )
            .await?;

        Ok(results
            .into_iter()
            .map(|r| SearchResult {
                id: r.id,
                content: r.content,
                score: r.score,
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct SearchConversationEmbeddingsCommand {
    pub deployment_id: i64,
    pub execution_context_id: i64,
    pub query_embedding: Vec<f32>,
    pub limit: u64,
}

impl SearchConversationEmbeddingsCommand {
    pub fn new(
        deployment_id: i64,
        execution_context_id: i64,
        query_embedding: Vec<f32>,
        limit: u64,
    ) -> Self {
        Self {
            deployment_id,
            execution_context_id,
            query_embedding,
            limit,
        }
    }
}

impl Command for SearchConversationEmbeddingsCommand {
    type Output = Vec<crate::services::clickhouse::MessageSearchResult>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .search_execution_messages(self.execution_context_id, self.query_embedding, self.limit)
            .await
    }
}

#[derive(Debug, Clone)]
pub struct SearchContextEmbeddingsCommand {
    pub deployment_id: i64,
    pub execution_context_id: i64,
    pub query_embedding: Vec<f32>,
    pub limit: u64,
    pub filters: Option<HashMap<String, serde_json::Value>>,
}

impl SearchContextEmbeddingsCommand {
    pub fn new(
        deployment_id: i64,
        execution_context_id: i64,
        query_embedding: Vec<f32>,
        limit: u64,
        filters: Option<HashMap<String, serde_json::Value>>,
    ) -> Self {
        Self {
            deployment_id,
            execution_context_id,
            query_embedding,
            limit,
            filters,
        }
    }
}

// Deletion commands for embeddings

/// Command to delete all knowledge base embeddings for a specific knowledge base
#[derive(Debug, Clone)]
pub struct DeleteKnowledgeBaseEmbeddingsCommand {
    pub knowledge_base_id: i64,
}

impl DeleteKnowledgeBaseEmbeddingsCommand {
    pub fn new(knowledge_base_id: i64) -> Self {
        Self { knowledge_base_id }
    }
}

impl Command for DeleteKnowledgeBaseEmbeddingsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .delete_knowledge_base_embeddings(self.knowledge_base_id)
            .await
    }
}

/// Command to delete document embeddings for a specific document
#[derive(Debug, Clone)]
pub struct DeleteDocumentEmbeddingsCommand {
    pub knowledge_base_id: i64,
    pub document_id: i64,
}

impl DeleteDocumentEmbeddingsCommand {
    pub fn new(knowledge_base_id: i64, document_id: i64) -> Self {
        Self {
            knowledge_base_id,
            document_id,
        }
    }
}

impl Command for DeleteDocumentEmbeddingsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .delete_document_embeddings(self.document_id)
            .await
    }
}

/// Command to delete execution context embeddings
#[derive(Debug, Clone)]
pub struct DeleteExecutionContextEmbeddingsCommand {
    pub execution_context_id: i64,
}

impl DeleteExecutionContextEmbeddingsCommand {
    pub fn new(execution_context_id: i64) -> Self {
        Self {
            execution_context_id,
        }
    }
}

impl Command for DeleteExecutionContextEmbeddingsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .delete_execution_context_embeddings(self.execution_context_id)
            .await
    }
}

/// Command to delete agent memories
#[derive(Debug, Clone)]
pub struct DeleteAgentMemoriesCommand {
    pub agent_id: i64,
}

impl DeleteAgentMemoriesCommand {
    pub fn new(agent_id: i64) -> Self {
        Self { agent_id }
    }
}

impl Command for DeleteAgentMemoriesCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .delete_agent_memories(self.agent_id)
            .await
    }
}

/// Command to delete execution context memories
#[derive(Debug, Clone)]
pub struct DeleteExecutionContextMemoriesCommand {
    pub execution_context_id: i64,
}

impl DeleteExecutionContextMemoriesCommand {
    pub fn new(execution_context_id: i64) -> Self {
        Self {
            execution_context_id,
        }
    }
}

impl Command for DeleteExecutionContextMemoriesCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        app_state
            .clickhouse_service
            .delete_execution_context_memories(self.execution_context_id)
            .await
    }
}
