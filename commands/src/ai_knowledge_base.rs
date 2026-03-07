use crate::{DispatchDocumentProcessingTaskCommand, WriteToAgentStorageCommand};
use common::error::AppError;
use models::{AiKnowledgeBase, AiKnowledgeBaseDocument};
use queries::GetAiKnowledgeBaseByIdQuery;

use chrono::Utc;
use sqlx::Row;

pub struct CreateAiKnowledgeBaseCommand {
    pub knowledge_base_id: Option<i64>,
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
            knowledge_base_id: None,
            deployment_id,
            name,
            description,
            configuration,
        }
    }

    pub fn with_knowledge_base_id(mut self, knowledge_base_id: i64) -> Self {
        self.knowledge_base_id = Some(knowledge_base_id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AiKnowledgeBase, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        validate_knowledge_base_name(&self.name)?;
        let knowledge_base_id = self.knowledge_base_id.ok_or_else(|| {
            AppError::Validation("knowledge_base_id is required".to_string())
        })?;
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
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

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

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<AiKnowledgeBase, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if let Some(ref name) = self.name {
            validate_knowledge_base_name(name)?;
        }
        let now = Utc::now();

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

        let mut query_builder = sqlx::query(&query).bind(now);
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
            .fetch_one(executor)
            .await
            .map_err(AppError::Database)?;

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

fn validate_knowledge_base_name(name: &str) -> Result<(), AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(
            "Knowledge base name cannot be empty".to_string(),
        ));
    }

    if trimmed == "." || trimmed == ".." {
        return Err(AppError::BadRequest(
            "Knowledge base name cannot be '.' or '..'".to_string(),
        ));
    }

    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(AppError::BadRequest(
            "Knowledge base name cannot contain path separators".to_string(),
        ));
    }

    if trimmed.chars().any(|ch| ch.is_control()) {
        return Err(AppError::BadRequest(
            "Knowledge base name cannot contain control characters".to_string(),
        ));
    }

    Ok(())
}

pub struct DeleteAiKnowledgeBaseCommand {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
}

pub struct KnowledgeBaseStorageDeps<'a> {
    pub db_router: &'a common::DbRouter,
    pub storage_client: &'a aws_sdk_s3::Client,
}

impl DeleteAiKnowledgeBaseCommand {
    pub fn new(deployment_id: i64, knowledge_base_id: i64) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
        }
    }

    pub async fn execute_with_deps(
        self,
        deps: KnowledgeBaseStorageDeps<'_>,
    ) -> Result<(), AppError> {
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
        .fetch_all(deps.db_router.writer())
        .await
        .map_err(AppError::Database)?;

        if !dependent_tools.is_empty() {
            let tool_names: Vec<String> = dependent_tools
                .iter()
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
            JOIN ai_agent_knowledge_bases aakb ON aakb.agent_id = a.id
            WHERE a.deployment_id = $1
            AND aakb.knowledge_base_id = $2
            AND aakb.deployment_id = $1
            "#,
            self.deployment_id,
            self.knowledge_base_id
        )
        .fetch_all(deps.db_router.writer())
        .await
        .map_err(AppError::Database)?;

        if !dependent_agents.is_empty() {
            let agent_names: Vec<String> = dependent_agents
                .iter()
                .map(|agent| agent.name.clone())
                .collect();
            return Err(AppError::BadRequest(format!(
                "Cannot delete knowledge base. The following agents depend on it: {}. Please remove this knowledge base from these agents first.",
                agent_names.join(", ")
            )));
        }

        let storage_prefix = format!(
            "{}/knowledge-bases/{}/",
            self.deployment_id, self.knowledge_base_id
        );
        if let Err(e) = crate::DeletePrefixFromAgentStorageCommand::new(storage_prefix)
            .execute_with_deps(deps.storage_client)
            .await
        {
            tracing::warn!(
                "Failed to clean storage for KB {}: {}",
                self.knowledge_base_id,
                e
            );
        }

        let mut tx = deps
            .db_router
            .writer()
            .begin()
            .await
            .map_err(AppError::Database)?;
        sqlx::query!(
            "DELETE FROM ai_knowledge_base_documents WHERE knowledge_base_id = $1",
            self.knowledge_base_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        sqlx::query!(
            "DELETE FROM ai_knowledge_bases WHERE id = $1 AND deployment_id = $2",
            self.knowledge_base_id,
            self.deployment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(AppError::Database)?;

        tx.commit().await.map_err(AppError::Database)?;
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
    pub document_id: Option<i64>,
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
            document_id: None,
        }
    }

    pub fn with_document_id(mut self, document_id: i64) -> Self {
        self.document_id = Some(document_id);
        self
    }

    pub async fn execute_with_deps(
        self,
        deps: UploadKnowledgeBaseDocumentDeps<'_>,
    ) -> Result<AiKnowledgeBaseDocument, AppError> {
        let document_id = self
            .document_id
            .ok_or_else(|| AppError::Validation("document_id is required".to_string()))?;
        let now = Utc::now();
        let file_size = self.file_content.len() as i64;

        let kb_query = sqlx::query!(
            "SELECT deployment_id FROM ai_knowledge_bases WHERE id = $1",
            self.knowledge_base_id
        )
        .fetch_one(deps.db_router.writer())
        .await?;
        let deployment_id = kb_query.deployment_id;

        let file_path = format!(
            "{}/knowledge-bases/{}/{}",
            deployment_id, self.knowledge_base_id, self.file_name
        );
        let file_url = WriteToAgentStorageCommand::new(file_path, self.file_content.clone())
            .with_content_type(self.file_type.clone())
            .execute_with_deps(deps.storage_client)
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;

        let document = sqlx::query!(
            r#"
            INSERT INTO ai_knowledge_base_documents
            (id, created_at, updated_at, title, description, file_name, file_size, file_type, file_url, knowledge_base_id, processing_metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
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
            file_url.clone(),
            self.knowledge_base_id,
            serde_json::json!({"status": "processing"})
        )
        .fetch_one(deps.db_router.writer())
        .await
        .map_err(AppError::Database)?;

        let dispatch_processing_task = DispatchDocumentProcessingTaskCommand::new(
            deployment_id,
            self.knowledge_base_id,
            document.id,
        );

        if let Err(e) = dispatch_processing_task.execute_with_deps(deps.nats_client).await {
            tracing::error!("Failed to dispatch document processing task: {}", e);
            let _ = sqlx::query!(
                r#"
                UPDATE ai_knowledge_base_documents 
                SET processing_metadata = jsonb_set(
                    COALESCE(processing_metadata, '{}'),
                    '{status}',
                    '"failed"'
                ),
                updated_at = $1
                WHERE id = $2
                "#,
                chrono::Utc::now(),
                document.id
            )
            .execute(deps.db_router.writer())
            .await;
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

    pub async fn execute_with_deps(
        self,
        deps: KnowledgeBaseStorageDeps<'_>,
    ) -> Result<(), AppError> {
        let _kb = GetAiKnowledgeBaseByIdQuery::new(self.deployment_id, self.knowledge_base_id)
            .execute_with_db(deps.db_router.writer())
            .await
            .map_err(|_| AppError::NotFound("Knowledge base not found".to_string()))?;

        let doc = sqlx::query!(
            "SELECT file_name FROM ai_knowledge_base_documents WHERE id = $1 AND knowledge_base_id = $2",
            self.document_id,
            self.knowledge_base_id
        )
        .fetch_optional(deps.db_router.writer())
        .await
        .map_err(AppError::Database)?
        .ok_or(AppError::NotFound("Document not found".to_string()))?;

        let storage_key = format!(
            "{}/knowledge-bases/{}/{}",
            self.deployment_id, self.knowledge_base_id, doc.file_name
        );
        if let Err(e) = crate::DeleteFromAgentStorageCommand::new(storage_key)
            .execute_with_deps(deps.storage_client)
            .await
        {
            tracing::warn!("Failed to delete file from storage: {}", e);
        }

        sqlx::query!(
            "DELETE FROM ai_knowledge_base_documents WHERE id = $1 AND knowledge_base_id = $2",
            self.document_id,
            self.knowledge_base_id
        )
        .execute(deps.db_router.writer())
        .await
        .map_err(AppError::Database)?;

        Ok(())
    }
}

pub struct UploadKnowledgeBaseDocumentDeps<'a> {
    pub db_router: &'a common::DbRouter,
    pub storage_client: &'a aws_sdk_s3::Client,
    pub nats_client: &'a async_nats::Client,
}
