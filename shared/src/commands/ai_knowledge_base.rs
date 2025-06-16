use crate::{
    commands::{Command, UploadToKnowledgeBaseBucketCommand},
    error::AppError,
    models::{AiKnowledgeBase, AiKnowledgeBaseDocument},
    queries::{GetAiKnowledgeBaseByIdQuery, Query},
    services::{
        embedding::EmbeddingService,
        qdrant::{DocumentChunk, QdrantService},
        text_processing::TextProcessingService,
    },
    state::AppState,
};
use chrono::Utc;
use serde_json::json;
use sqlx::Row;
use std::collections::HashMap;

pub struct CreateAiKnowledgeBaseCommand {
    pub deployment_id: i64,
    pub name: String,
    pub description: Option<String>,
    pub configuration: serde_json::Value,
}

impl CreateAiKnowledgeBaseCommand {
    pub fn new(
        deployment_id: i64,
        name: String,
        description: Option<String>,
        configuration: serde_json::Value,
    ) -> Self {
        Self {
            deployment_id,
            name,
            description,
            configuration,
        }
    }
}

impl Command for CreateAiKnowledgeBaseCommand {
    type Output = AiKnowledgeBase;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let knowledge_base_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        let knowledge_base = sqlx::query!(
            r#"
            INSERT INTO ai_knowledge_bases (id, created_at, updated_at, name, description, deployment_id, configuration)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration
            "#,
            knowledge_base_id,
            now,
            now,
            self.name,
            self.description,
            self.deployment_id,
            self.configuration,
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AiKnowledgeBase {
            id: knowledge_base.id,
            created_at: knowledge_base.created_at,
            updated_at: knowledge_base.updated_at,
            name: knowledge_base.name,
            description: knowledge_base.description,
            deployment_id: knowledge_base.deployment_id,
            configuration: knowledge_base.configuration,
        })
    }
}

pub struct UpdateAiKnowledgeBaseCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub name: Option<String>,
    pub description: Option<String>,
    pub configuration: Option<serde_json::Value>,
}

impl UpdateAiKnowledgeBaseCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
            name: None,
            description: None,
            configuration: None,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = Some(name);
        self
    }

    pub fn with_description(mut self, description: Option<String>) -> Self {
        self.description = description;
        self
    }

    pub fn with_configuration(mut self, configuration: serde_json::Value) -> Self {
        self.configuration = Some(configuration);
        self
    }
}

impl Command for UpdateAiKnowledgeBaseCommand {
    type Output = AiKnowledgeBase;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        // Build dynamic query based on provided fields
        let mut query_parts = vec!["updated_at = $1".to_string()];
        let mut param_count = 2;

        if self.name.is_some() {
            query_parts.push(format!("name = ${}", param_count));
            param_count += 1;
        }
        if self.description.is_some() {
            query_parts.push(format!("description = ${}", param_count));
            param_count += 1;
        }

        if self.configuration.is_some() {
            query_parts.push(format!("configuration = ${}", param_count));
            param_count += 1;
        }

        let query = format!(
            r#"
            UPDATE ai_knowledge_bases 
            SET {}
            WHERE id = ${} AND deployment_id = ${}
            RETURNING id, created_at, updated_at, name, description, deployment_id, configuration
            "#,
            query_parts.join(", "),
            param_count,
            param_count + 1
        );

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(now);

        if let Some(name) = self.name {
            query_builder = query_builder.bind(name);
        }
        if let Some(description) = self.description {
            query_builder = query_builder.bind(description);
        }

        if let Some(configuration) = self.configuration {
            query_builder = query_builder.bind(configuration);
        }

        query_builder = query_builder
            .bind(self.knowledge_base_id)
            .bind(self.deployment_id);

        let knowledge_base = query_builder
            .fetch_one(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(AiKnowledgeBase {
            id: knowledge_base.get("id"),
            created_at: knowledge_base.get("created_at"),
            updated_at: knowledge_base.get("updated_at"),
            name: knowledge_base.get("name"),
            description: knowledge_base.get("description"),
            deployment_id: knowledge_base.get("deployment_id"),
            configuration: knowledge_base.get("configuration"),
        })
    }
}

pub struct DeleteAiKnowledgeBaseCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
}

impl DeleteAiKnowledgeBaseCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
        }
    }
}

impl Command for DeleteAiKnowledgeBaseCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Check if any tools depend on this knowledge base
        let dependent_tools = sqlx::query!(
            r#"
            SELECT t.id, t.name
            FROM ai_tools t
            WHERE t.deployment_id = $1
            AND t.tool_type = 'knowledge_base'
            AND t.configuration->>'knowledge_base_id' = $2::text
            "#,
            self.deployment_id,
            self.knowledge_base_id.to_string()
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        if !dependent_tools.is_empty() {
            let tool_names: Vec<String> = dependent_tools.iter()
                .map(|tool| tool.name.clone())
                .collect();

            return Err(AppError::BadRequest(format!(
                "Cannot delete knowledge base. The following tools depend on it: {}. Please delete or update these tools first.",
                tool_names.join(", ")
            )));
        }

        let dependent_agents = sqlx::query!(
            r#"
            SELECT a.id, a.name
            FROM ai_agents a
            WHERE a.deployment_id = $1
            AND a.configuration->'knowledge_base_ids' ? $2::text
            "#,
            self.deployment_id,
            self.knowledge_base_id.to_string()
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        if !dependent_agents.is_empty() {
            let agent_names: Vec<String> = dependent_agents.iter()
                .map(|agent| agent.name.clone())
                .collect();

            return Err(AppError::BadRequest(format!(
                "Cannot delete knowledge base. The following agents depend on it: {}. Please remove this knowledge base from these agents first.",
                agent_names.join(", ")
            )));
        }

        let mut tx = app_state
            .db_pool
            .begin()
            .await
            .map_err(|e| AppError::Database(e))?;

        // Delete all documents first
        sqlx::query!(
            "DELETE FROM ai_knowledge_base_documents WHERE knowledge_base_id = $1",
            self.knowledge_base_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Delete the knowledge base
        sqlx::query!(
            "DELETE FROM ai_knowledge_bases WHERE id = $1 AND deployment_id = $2",
            self.knowledge_base_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e))?;

        tx.commit().await.map_err(|e| AppError::Database(e))?;

        // Delete Qdrant collection in background
        let kb_id = self.knowledge_base_id;

        if let Err(e) = QdrantService::delete_knowledge_base(kb_id).await {
            eprintln!(
                "Failed to delete Qdrant vectors for knowledge base {}: {}",
                kb_id, e
            );
        }

        Ok(())
    }
}

pub struct UploadKnowledgeBaseDocumentCommand {
    pub knowledge_base_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub file_name: String,
    pub file_content: Vec<u8>,
    pub file_type: String,
}

pub struct UploadKnowledgeBaseUrlCommand {
    pub knowledge_base_id: i64,
    pub title: String,
    pub description: Option<String>,
    pub url: String,
}

impl UploadKnowledgeBaseDocumentCommand {
    pub fn new(
        knowledge_base_id: i64,
        title: String,
        description: Option<String>,
        file_name: String,
        file_content: Vec<u8>,
        file_type: String,
    ) -> Self {
        Self {
            knowledge_base_id,
            title,
            description,
            file_name,
            file_content,
            file_type,
        }
    }
}

impl UploadKnowledgeBaseUrlCommand {
    pub fn new(
        knowledge_base_id: i64,
        title: String,
        description: Option<String>,
        url: String,
    ) -> Self {
        Self {
            knowledge_base_id,
            title,
            description,
            url,
        }
    }
}

impl Command for UploadKnowledgeBaseDocumentCommand {
    type Output = AiKnowledgeBaseDocument;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let document_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();
        let file_size = self.file_content.len() as i64;

        // Upload file to knowledge base bucket (directly in root)
        let file_path = self.file_name.clone();
        let file_content_clone = self.file_content.clone();
        let file_url = UploadToKnowledgeBaseBucketCommand::new(file_path, file_content_clone)
            .execute(app_state)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let document = sqlx::query!(
            r#"
            INSERT INTO ai_knowledge_base_documents
            (id, created_at, updated_at, title, description, file_name, file_size, file_type, file_url, knowledge_base_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING id, created_at, updated_at, title, description, file_name, file_size, file_type, file_url, knowledge_base_id, processing_metadata
            "#,
            document_id,
            now,
            now,
            self.title,
            self.description,
            self.file_name,
            file_size,
            self.file_type,
            file_url,
            self.knowledge_base_id,
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Process document for embeddings in background
        let doc_id = document.id;
        let file_content_for_processing = self.file_content.clone();
        let file_type_clone = self.file_type.clone();
        let title_clone = self.title.clone();
        let kb_id = self.knowledge_base_id;

        // Clone services from app_state for background processing
        let embedding_service = app_state.embedding_service.clone();
        let text_processing_service = app_state.text_processing_service.clone();

        if let Err(e) = Self::process_document_embeddings(
            doc_id,
            kb_id,
            file_content_for_processing,
            file_type_clone,
            title_clone,
            embedding_service,
            text_processing_service,
        )
        .await
        {
            eprintln!("Failed to process document embeddings: {}", e);
        }

        Ok(AiKnowledgeBaseDocument {
            id: document.id,
            created_at: document.created_at,
            updated_at: document.updated_at,
            title: document.title,
            description: document.description,
            file_name: document.file_name,
            file_size: document.file_size,
            file_type: document.file_type,
            file_url: document.file_url,
            knowledge_base_id: document.knowledge_base_id,
            processing_metadata: document.processing_metadata,
        })
    }
}

impl UploadKnowledgeBaseDocumentCommand {
    async fn process_document_embeddings(
        document_id: i64,
        knowledge_base_id: i64,
        file_content: Vec<u8>,
        file_type: String,
        title: String,
        embedding_service: EmbeddingService,
        text_processing_service: TextProcessingService,
    ) -> Result<(), AppError> {
        // Create a snowflake generator for chunk IDs
        let sf = sonyflake::Sonyflake::new().map_err(|e| AppError::Internal(e.to_string()))?;
        // Extract text from file
        let text = text_processing_service.extract_text_from_file(&file_content, &file_type)?;
        let cleaned_text = text_processing_service.clean_text(&text);

        // Chunk the text
        let chunks = text_processing_service.chunk_text(&cleaned_text, 1000, 200)?;

        // Generate embeddings for chunks
        let chunk_texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        let embeddings = embedding_service.generate_embeddings(chunk_texts).await?;

        // Create document chunks for Qdrant
        let document_chunks: Vec<DocumentChunk> = chunks
            .into_iter()
            .zip(embeddings.into_iter())
            .enumerate()
            .map(|(_i, (chunk, embedding))| {
                let mut metadata = HashMap::new();
                metadata.insert("document_id".to_string(), json!(document_id.to_string()));
                metadata.insert(
                    "knowledge_base_id".to_string(),
                    json!(knowledge_base_id.to_string()),
                );
                metadata.insert("chunk_index".to_string(), json!(chunk.chunk_index));
                metadata.insert("title".to_string(), json!(title.clone()));
                metadata.insert("file_type".to_string(), json!(file_type.clone()));

                // Generate consistent chunk ID using snowflake
                let chunk_id = sf.next_id().unwrap_or(0) as i64;

                DocumentChunk {
                    id: chunk_id, // Use snowflake ID as i64
                    content: chunk.content,
                    metadata,
                    embedding,
                }
            })
            .collect();

        // Store in Qdrant with knowledge base as tenant
        QdrantService::upsert_documents(document_chunks, knowledge_base_id).await?;

        Ok(())
    }
}

pub struct DeleteKnowledgeBaseDocumentCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub document_id: i64,
}

impl DeleteKnowledgeBaseDocumentCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64, document_id: i64) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
            document_id,
        }
    }
}

impl Command for DeleteKnowledgeBaseDocumentCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Verify the knowledge base exists and belongs to the deployment
        let _kb = GetAiKnowledgeBaseByIdQuery::new(self.deployment_id, self.knowledge_base_id)
            .execute(app_state)
            .await
            .map_err(|_| AppError::NotFound("Knowledge base not found".to_string()))?;

        // Delete the document
        let result = sqlx::query!(
            "DELETE FROM ai_knowledge_base_documents WHERE id = $1 AND knowledge_base_id = $2",
            self.document_id,
            self.knowledge_base_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("Document not found".to_string()));
        }

        // Delete embeddings from Qdrant in background
        let doc_id = self.document_id;
        let kb_id = self.knowledge_base_id;

        if let Err(e) = Self::delete_document_embeddings(doc_id, kb_id).await {
            eprintln!("Failed to delete document embeddings: {}", e);
        }

        Ok(())
    }
}

impl DeleteKnowledgeBaseDocumentCommand {
    async fn delete_document_embeddings(
        document_id: i64,
        knowledge_base_id: i64,
    ) -> Result<(), AppError> {
        let mut metadata_filter = HashMap::new();
        metadata_filter.insert("document_id".to_string(), json!(document_id.to_string()));

        QdrantService::delete_by_metadata(knowledge_base_id, metadata_filter).await?;

        Ok(())
    }
}

impl Command for UploadKnowledgeBaseUrlCommand {
    type Output = AiKnowledgeBaseDocument;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        // Fetch content from URL using ureq
        let mut response = ureq::get(&self.url)
            .call()
            .map_err(|e| AppError::Internal(format!("Failed to fetch URL: {}", e)))?;

        if response.status() != 200 {
            return Err(AppError::Internal(format!(
                "Failed to fetch URL: HTTP {}",
                response.status()
            )));
        }

        // For now, we'll determine content type from URL extension or default to text/html
        let content_type = if self.url.ends_with(".pdf") {
            "application/pdf"
        } else if self.url.ends_with(".json") {
            "application/json"
        } else if self.url.ends_with(".xml") {
            "application/xml"
        } else {
            "text/html"
        }
        .to_string();

        // Read the response body as string and convert to bytes
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| AppError::Internal(format!("Failed to read URL content: {}", e)))?;

        let content = text.into_bytes();

        // Extract filename from URL
        let file_name = self.url.split('/').last().unwrap_or("webpage").to_string();

        // Create document upload command with fetched content
        let upload_command = UploadKnowledgeBaseDocumentCommand::new(
            self.knowledge_base_id,
            self.title,
            self.description,
            file_name,
            content.to_vec(),
            content_type,
        );

        upload_command.execute(app_state).await
    }
}
