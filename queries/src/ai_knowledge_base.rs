use sqlx::Row;

use common::error::AppError;
use models::{AiKnowledgeBase, AiKnowledgeBaseDocument, AiKnowledgeBaseWithDetails};
use crate::Query;
use common::state::AppState;

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
}

impl Query for GetAiKnowledgeBasesQuery {
    type Output = Vec<AiKnowledgeBaseWithDetails>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
            let query_with_search = format!("{} AND (kb.name ILIKE $2 OR kb.description ILIKE $2) ORDER BY kb.created_at DESC LIMIT $3 OFFSET $4", base_query);
            sqlx::query(&query_with_search)
                .bind(self.deployment_id)
                .bind(format!("%{}%", search))
                .bind(self.limit as i64)
                .bind(self.offset as i64)
                .fetch_all(&app_state.db_pool)
                .await
        } else {
            let query_without_search = format!("{} ORDER BY kb.created_at DESC LIMIT $2 OFFSET $3", base_query);
            sqlx::query(&query_without_search)
                .bind(self.deployment_id)
                .bind(self.limit as i64)
                .bind(self.offset as i64)
                .fetch_all(&app_state.db_pool)
                .await
        }
        .map_err(|e| AppError::Database(e))?;

        Ok(knowledge_bases
            .into_iter()
            .map(|row| AiKnowledgeBaseWithDetails {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                name: row.get("name"),
                description: row.get("description"),
                configuration: row.get("configuration"),
                deployment_id: row.get("deployment_id"),
                documents_count: row.get::<Option<i64>, _>("documents_count").unwrap_or(0),
                total_size: row.get("total_size"),
            })
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
}

impl Query for GetAiKnowledgeBaseByIdQuery {
    type Output = AiKnowledgeBaseWithDetails;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AiKnowledgeBaseWithDetails {
            id: knowledge_base.get("id"),
            created_at: knowledge_base.get("created_at"),
            updated_at: knowledge_base.get("updated_at"),
            name: knowledge_base.get("name"),
            description: knowledge_base.get("description"),
            configuration: knowledge_base.get("configuration"),
            deployment_id: knowledge_base.get("deployment_id"),
            documents_count: knowledge_base
                .get::<Option<i64>, _>("documents_count")
                .unwrap_or(0),
            total_size: knowledge_base.get("total_size"),
        })
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
}

impl Query for GetKnowledgeBaseDocumentsQuery {
    type Output = Vec<AiKnowledgeBaseDocument>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let documents = sqlx::query!(
            r#"
            SELECT
                id, created_at, updated_at, title, description, file_name,
                file_size, file_type, file_url, knowledge_base_id,
                processing_metadata
            FROM ai_knowledge_base_documents
            WHERE knowledge_base_id = $1
            ORDER BY created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            self.knowledge_base_id,
            self.limit as i64,
            self.offset as i64
        )
        .fetch_all(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(documents
            .into_iter()
            .map(|row| AiKnowledgeBaseDocument {
                id: row.id,
                created_at: row.created_at,
                updated_at: row.updated_at,
                title: row.title,
                description: row.description,
                file_name: row.file_name,
                file_size: row.file_size,
                file_type: row.file_type,
                file_url: row.file_url,
                knowledge_base_id: row.knowledge_base_id,
                processing_metadata: row.processing_metadata,
            })
            .collect())
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
}

impl Query for GetAiKnowledgeBasesByIdsQuery {
    type Output = Vec<AiKnowledgeBase>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
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
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        Ok(knowledge_bases
            .into_iter()
            .map(|row| AiKnowledgeBase {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                name: row.get("name"),
                description: row.get("description"),
                deployment_id: row.get("deployment_id"),
                configuration: row.get("configuration"),
            })
            .collect())
    }
}

pub struct GetDocumentChunksQuery {
    pub document_id: i64,
    pub chunk_range: Option<(i32, i32)>,
    pub keywords: Option<Vec<String>>,
    pub limit: Option<usize>,
}

impl GetDocumentChunksQuery {
    pub fn new(document_id: i64) -> Self {
        Self {
            document_id,
            chunk_range: None,
            keywords: None,
            limit: Some(10),
        }
    }

    pub fn with_chunk_range(mut self, start: i32, end: i32) -> Self {
        self.chunk_range = Some((start, end));
        self
    }

    pub fn with_keywords(mut self, keywords: Vec<String>) -> Self {
        self.keywords = Some(keywords);
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

#[derive(Debug)]
pub struct DocumentChunk {
    pub content: String,
    pub chunk_index: i32,
    pub knowledge_base_id: i64,
    pub deployment_id: i64,
}

impl Query for GetDocumentChunksQuery {
    type Output = Vec<DocumentChunk>;

    async fn execute(&self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut query_str = String::from(
            "SELECT content, chunk_index, knowledge_base_id, deployment_id
             FROM knowledge_base_document_chunks
             WHERE document_id = $1",
        );

        let mut param_count = 1;

        if let Some((start, end)) = self.chunk_range {
            param_count += 1;
            query_str.push_str(&format!(" AND chunk_index >= ${}", start));
            param_count += 1;
            query_str.push_str(&format!(" AND chunk_index <= ${}", end));
        }

        if self.keywords.is_some() {
            param_count += 1;
            query_str.push_str(&format!(" AND content ~* ${}", param_count));
        }

        query_str.push_str(" ORDER BY chunk_index");

        param_count += 1;
        query_str.push_str(&format!(" LIMIT ${}", param_count));

        let mut query = sqlx::query(&query_str);
        query = query.bind(self.document_id);

        if let Some((start, end)) = self.chunk_range {
            query = query.bind(start);
            query = query.bind(end);
        }

        if let Some(keywords) = &self.keywords {
            let keyword_pattern = keywords.join("|");
            query = query.bind(keyword_pattern);
        }

        query = query.bind(self.limit.unwrap_or(10) as i64);

        let rows = query
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

        let mut chunks = Vec::new();
        for row in rows {
            chunks.push(DocumentChunk {
                content: sqlx::Row::try_get(&row, "content")?,
                chunk_index: sqlx::Row::try_get(&row, "chunk_index")?,
                knowledge_base_id: sqlx::Row::try_get(&row, "knowledge_base_id")?,
                deployment_id: sqlx::Row::try_get(&row, "deployment_id")?,
            });
        }

        Ok(chunks)
    }
}
