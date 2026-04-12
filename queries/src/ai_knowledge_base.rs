use sqlx::Row;

use common::error::AppError;
use models::{AiKnowledgeBase, AiKnowledgeBaseDocument, AiKnowledgeBaseWithDetails};

fn map_knowledge_base_with_details(row: sqlx::postgres::PgRow) -> AiKnowledgeBaseWithDetails {
    AiKnowledgeBaseWithDetails {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        name: row.get("name"),
        description: row.get("description"),
        configuration: row.get("configuration"),
        deployment_id: row.get("deployment_id"),
        documents_count: row.get::<Option<i64>, _>("documents_count").unwrap_or(0),
        total_size: row.get("total_size"),
    }
}

fn map_knowledge_base(row: sqlx::postgres::PgRow) -> AiKnowledgeBase {
    AiKnowledgeBase {
        id: row.get("id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        name: row.get("name"),
        description: row.get("description"),
        deployment_id: row.get("deployment_id"),
        configuration: row.get("configuration"),
    }
}

pub struct GetAiKnowledgeBasesQuery {
    pub deployment_id: i64,
    pub limit: usize,
    pub offset: usize,
    pub search: Option<String>,
}

impl GetAiKnowledgeBasesQuery {
    pub fn new(deployment_id: i64, limit: usize, offset: usize) -> Self {
        Self {
            deployment_id,
            limit,
            offset,
            search: None,
        }
    }

    pub fn with_search(mut self, search: String) -> Self {
        self.search = Some(search);
        self
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<AiKnowledgeBaseWithDetails>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let base_query = r#"
            SELECT
                kb.id, kb.created_at, kb.updated_at, kb.name, kb.description,
                kb.configuration, kb.deployment_id,
                COALESCE(d.documents_count, 0) as documents_count,
                COALESCE(d.total_size, 0) as total_size
            FROM ai_knowledge_bases kb
            LEFT JOIN (
                SELECT knowledge_base_id, COUNT(*) as documents_count, COALESCE(SUM(file_size), 0)::bigint as total_size
                FROM ai_knowledge_base_documents
                GROUP BY knowledge_base_id
            ) d ON kb.id = d.knowledge_base_id
            WHERE kb.deployment_id = $1"#;

        let knowledge_bases = if let Some(search) = &self.search {
            let query_with_search = format!(
                "{} AND (kb.name ILIKE $2 OR kb.description ILIKE $2) ORDER BY kb.created_at DESC LIMIT $3 OFFSET $4",
                base_query
            );
            sqlx::query(&query_with_search)
                .bind(self.deployment_id)
                .bind(format!("%{}%", search))
                .bind(self.limit as i64)
                .bind(self.offset as i64)
                .fetch_all(executor)
                .await
        } else {
            let query_without_search = format!(
                "{} ORDER BY kb.created_at DESC LIMIT $2 OFFSET $3",
                base_query
            );
            sqlx::query(&query_without_search)
                .bind(self.deployment_id)
                .bind(self.limit as i64)
                .bind(self.offset as i64)
                .fetch_all(executor)
                .await
        }
        .map_err(AppError::Database)?;

        Ok(knowledge_bases
            .into_iter()
            .map(map_knowledge_base_with_details)
            .collect())
    }
}

pub struct GetAiKnowledgeBaseByIdQuery {
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
}

impl GetAiKnowledgeBaseByIdQuery {
    pub fn new(deployment_id: i64, knowledge_base_id: i64) -> Self {
        Self {
            deployment_id,
            knowledge_base_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<AiKnowledgeBaseWithDetails, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let knowledge_base = sqlx::query(
            r#"
            SELECT
                kb.id, kb.created_at, kb.updated_at, kb.name, kb.description,
                kb.configuration, kb.deployment_id,
                COALESCE(d.documents_count, 0) as documents_count,
                COALESCE(d.total_size, 0) as total_size
            FROM ai_knowledge_bases kb
            LEFT JOIN (
                SELECT knowledge_base_id, COUNT(*) as documents_count, COALESCE(SUM(file_size), 0)::bigint as total_size
                FROM ai_knowledge_base_documents
                GROUP BY knowledge_base_id
            ) d ON kb.id = d.knowledge_base_id
            WHERE kb.id = $1 AND kb.deployment_id = $2
            "#
        )
        .bind(self.knowledge_base_id)
        .bind(self.deployment_id)
        .fetch_one(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(map_knowledge_base_with_details(knowledge_base))
    }
}

pub struct GetAgentKnowledgeBasesQuery {
    pub deployment_id: i64,
    pub agent_id: i64,
}

impl GetAgentKnowledgeBasesQuery {
    pub fn new(deployment_id: i64, agent_id: i64) -> Self {
        Self {
            deployment_id,
            agent_id,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<AiKnowledgeBaseWithDetails>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let knowledge_bases = sqlx::query(
            r#"
            SELECT
                kb.id, kb.created_at, kb.updated_at, kb.name, kb.description,
                kb.configuration, kb.deployment_id,
                COALESCE(d.documents_count, 0) as documents_count,
                COALESCE(d.total_size, 0) as total_size
            FROM ai_knowledge_bases kb
            JOIN ai_agent_knowledge_bases aakb ON aakb.knowledge_base_id = kb.id
            LEFT JOIN (
                SELECT knowledge_base_id, COUNT(*) as documents_count, COALESCE(SUM(file_size), 0)::bigint as total_size
                FROM ai_knowledge_base_documents
                GROUP BY knowledge_base_id
            ) d ON kb.id = d.knowledge_base_id
            WHERE kb.deployment_id = $1 AND aakb.agent_id = $2 AND aakb.deployment_id = $1
            ORDER BY kb.created_at DESC
            "#,
        )
        .bind(self.deployment_id)
        .bind(self.agent_id)
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(knowledge_bases
            .into_iter()
            .map(map_knowledge_base_with_details)
            .collect())
    }
}

pub struct GetKnowledgeBaseDocumentsQuery {
    pub knowledge_base_id: i64,
    pub limit: usize,
    pub offset: usize,
}

impl GetKnowledgeBaseDocumentsQuery {
    pub fn new(knowledge_base_id: i64, limit: usize, offset: usize) -> Self {
        Self {
            knowledge_base_id,
            limit,
            offset,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<AiKnowledgeBaseDocument>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let documents = sqlx::query(
            r#"
            SELECT
                id, created_at, updated_at, title, description, file_name,
                file_size, file_type, storage_object_key, knowledge_base_id,
                processing_metadata
            FROM ai_knowledge_base_documents
            WHERE knowledge_base_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(self.knowledge_base_id)
        .bind(self.limit as i64)
        .bind(self.offset as i64)
        .fetch_all(executor)
        .await
        .map_err(AppError::Database)?;

        Ok(documents
            .into_iter()
            .map(|row| {
                Ok(AiKnowledgeBaseDocument {
                    id: row.try_get("id").map_err(AppError::Database)?,
                    created_at: row.try_get("created_at").map_err(AppError::Database)?,
                    updated_at: row.try_get("updated_at").map_err(AppError::Database)?,
                    title: row.try_get("title").map_err(AppError::Database)?,
                    description: row.try_get("description").map_err(AppError::Database)?,
                    file_name: row.try_get("file_name").map_err(AppError::Database)?,
                    file_size: row.try_get("file_size").map_err(AppError::Database)?,
                    file_type: row.try_get("file_type").map_err(AppError::Database)?,
                    storage_object_key: row
                        .try_get("storage_object_key")
                        .map_err(AppError::Database)?,
                    knowledge_base_id: row
                        .try_get("knowledge_base_id")
                        .map_err(AppError::Database)?,
                    processing_metadata: row
                        .try_get("processing_metadata")
                        .map_err(AppError::Database)?,
                })
            })
            .collect::<Result<Vec<_>, AppError>>()?)
    }
}

pub struct GetAiKnowledgeBasesByIdsQuery {
    pub deployment_id: i64,
    pub knowledge_base_ids: Vec<i64>,
}

impl GetAiKnowledgeBasesByIdsQuery {
    pub fn new(deployment_id: i64, knowledge_base_ids: Vec<i64>) -> Self {
        Self {
            deployment_id,
            knowledge_base_ids,
        }
    }

    pub async fn execute_with_db<'e, E>(
        &self,
        executor: E,
    ) -> Result<Vec<AiKnowledgeBase>, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        if self.knowledge_base_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders = (1..=self.knowledge_base_ids.len())
            .map(|i| format!("${}", i + 1))
            .collect::<Vec<_>>()
            .join(",");

        let query = format!(
            "SELECT id, created_at, updated_at, name, description, deployment_id, configuration
             FROM ai_knowledge_bases
             WHERE deployment_id = $1 AND id IN ({})",
            placeholders
        );

        let mut query_builder = sqlx::query(&query);
        query_builder = query_builder.bind(self.deployment_id);

        for kb_id in &self.knowledge_base_ids {
            query_builder = query_builder.bind(kb_id);
        }
        let knowledge_bases = query_builder
            .fetch_all(executor)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(knowledge_bases
            .into_iter()
            .map(map_knowledge_base)
            .collect())
    }
}

#[derive(Debug)]
pub struct DocumentChunk {
    pub content: String,
    pub chunk_index: i32,
    pub knowledge_base_id: i64,
    pub deployment_id: i64,
}
