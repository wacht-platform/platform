use common::{
    HasDbRouter, HasEmbeddingProvider, HasEncryptionService,
    db_router::ReadConsistency,
    error::AppError,
};
use models::ai_knowledge_base::DocumentChunkSearchResult;

use pgvector::HalfVector;
use sqlx::Row;

async fn resolve_deployment_gemini_api_key<D>(
    deps: &D,
    deployment_id: i64,
) -> Result<Option<String>, AppError>
where
    D: HasDbRouter + HasEncryptionService + ?Sized,
{
    let reader = deps.db_router().reader(ReadConsistency::Strong);
    let settings = queries::GetDeploymentAiSettingsQuery::new(deployment_id)
        .execute_with_db(reader)
        .await?;

    match settings.and_then(|s| s.gemini_api_key) {
        Some(encrypted) if !encrypted.is_empty() => {
            Ok(Some(deps.encryption_service().decrypt(&encrypted)?))
        }
        _ => Ok(None),
    }
}

#[derive(Clone)]
pub struct GenerateEmbeddingCommand {
    pub text: String,
    pub task_type: Option<String>,
    pub deployment_id: Option<i64>,
}

impl GenerateEmbeddingCommand {
    pub fn new(text: String) -> Self {
        Self {
            text,
            task_type: None,
            deployment_id: None,
        }
    }

    pub fn with_task_type(mut self, task_type: String) -> Self {
        self.task_type = Some(task_type);
        self
    }

    pub fn for_deployment(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<f32>, AppError>
    where
        D: HasEmbeddingProvider + HasDbRouter + HasEncryptionService + ?Sized,
    {
        let api_key_override = if let Some(deployment_id) = self.deployment_id {
            resolve_deployment_gemini_api_key(deps, deployment_id).await?
        } else {
            None
        };

        deps.embedding_provider()
            .embed_content(
                self.text,
                self.task_type.or(Some("RETRIEVAL_DOCUMENT".to_string())),
                Some(3072),
                api_key_override.as_deref(),
            )
            .await
    }
}

#[derive(Clone)]
pub struct GenerateEmbeddingsCommand {
    pub texts: Vec<String>,
    pub task_type: Option<String>,
    pub deployment_id: Option<i64>,
}

impl GenerateEmbeddingsCommand {
    pub fn new(texts: Vec<String>) -> Self {
        Self {
            texts,
            task_type: None,
            deployment_id: None,
        }
    }

    pub fn with_task_type(mut self, task_type: String) -> Self {
        self.task_type = Some(task_type);
        self
    }

    pub fn for_deployment(mut self, deployment_id: i64) -> Self {
        self.deployment_id = Some(deployment_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<Vec<Vec<f32>>, AppError>
    where
        D: HasEmbeddingProvider + HasDbRouter + HasEncryptionService + ?Sized,
    {
        let api_key_override = if let Some(deployment_id) = self.deployment_id {
            resolve_deployment_gemini_api_key(deps, deployment_id).await?
        } else {
            None
        };

        deps.embedding_provider()
            .batch_embed_contents(
                self.texts,
                self.task_type.or(Some("RETRIEVAL_DOCUMENT".to_string())),
                Some(3072),
                api_key_override.as_deref(),
            )
            .await
    }
}

#[derive(Clone)]
pub struct SearchKnowledgeBaseEmbeddingsCommand {
    pub knowledge_base_ids: Vec<i64>,
    pub query_embedding: Vec<f32>,
    pub limit: u64,
}

impl SearchKnowledgeBaseEmbeddingsCommand {
    pub fn new(knowledge_base_ids: Vec<i64>, query_embedding: Vec<f32>, limit: u64) -> Self {
        Self {
            knowledge_base_ids,
            query_embedding,
            limit,
        }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<Vec<DocumentChunkSearchResult>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let query_embedding = HalfVector::from_f32_slice(&self.query_embedding);
        let max_distance = 1.2_f64;

        let rows = sqlx::query(
            r#"
            SELECT
                kbc.document_id,
                kbc.knowledge_base_id,
                kbc.content,
                kbc.chunk_index,
                (kbc.embedding::vector(3072) <-> $1)::float8 as score,
                d.title as document_title,
                d.description as document_description
            FROM knowledge_base_document_chunks kbc
            LEFT JOIN ai_knowledge_base_documents d ON kbc.document_id = d.id
            WHERE kbc.knowledge_base_id = ANY($2)
              AND (kbc.embedding::vector(3072) <-> $1) <= $4
            ORDER BY score ASC
            LIMIT $3
            "#,
        )
        .bind(query_embedding)
        .bind(&self.knowledge_base_ids)
        .bind(self.limit as i64)
        .bind(max_distance)
        .fetch_all(executor)
        .await
        .map_err(AppError::from)?;

        let mut results = Vec::new();
        for row in rows {
            results.push(DocumentChunkSearchResult {
                document_id: row.try_get("document_id").map_err(AppError::from)?,
                knowledge_base_id: row.try_get("knowledge_base_id").map_err(AppError::from)?,
                content: row.try_get("content").map_err(AppError::from)?,
                score: row.try_get("score").map_err(AppError::from)?,
                chunk_index: row.try_get("chunk_index").map_err(AppError::from)?,
                document_title: row.try_get("document_title").map_err(AppError::from)?,
                document_description: row
                    .try_get("document_description")
                    .map_err(AppError::from)?,
            });
        }

        Ok(results)
    }
}
