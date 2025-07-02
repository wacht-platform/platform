use crate::error::AppError;
use chrono::{DateTime, Utc};
use clickhouse::{Client, Row};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct ClickHouseService {
    client: Client,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct UserEvent {
    pub deployment_id: i64,
    pub user_id: Option<i64>,
    pub event_type: String,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub auth_method: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub ip_address: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct CountResult {
    count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RecentSignup {
    pub name: Option<String>,
    pub email: Option<String>,
    pub method: Option<String>,
    pub date: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
struct RecentSignupRow {
    user_name: Option<String>,
    user_email: Option<String>,
    auth_method: Option<String>,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct KnowledgeBaseDocument {
    pub id: i64,
    pub deployment_id: i64,
    pub knowledge_base_id: i64,
    pub chunk_index: i32,
    pub content: String,
    pub embedding: Vec<f32>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct ExecutionMessage {
    pub id: i64,
    pub deployment_id: i64,
    pub execution_context_id: i64,
    pub agent_id: i64,
    pub message_type: String,
    pub content: String,
    pub embedding: Vec<f32>,
    #[serde(with = "clickhouse::serde::chrono::datetime64::millis")]
    pub created_at: DateTime<Utc>,
}

pub use crate::models::MemoryRecord as Memory;

#[derive(Debug, Serialize, Deserialize, Row)]
pub struct AnalyticsEvent {
    pub id: i64,
    pub deployment_id: i64,
    pub user_id: Option<i64>,
    pub event_type: String,
    pub event_data: String,
    pub embedding: Option<Vec<f32>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentSearchResult {
    pub id: i64,
    pub content: String,
    pub score: f32,
    pub knowledge_base_id: i64,
    pub chunk_index: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageSearchResult {
    pub id: i64,
    pub content: String,
    pub score: f32,
    pub execution_context_id: i64,
    pub agent_id: i64,
    pub message_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub id: i64,
    pub content: String,
    pub score: f32,
    pub agent_id: i64,
    pub memory_type: String,
    pub importance: f32,
    pub access_count: i32,
}

impl ClickHouseService {
    pub fn new(url: String, password: String) -> Result<Self, AppError> {
        let client = Client::default()
            .with_url(url)
            .with_user("default")
            .with_password(password)
            .with_database("wacht_prod");

        Ok(Self { client })
    }

    pub async fn init_tables(&self) -> Result<(), AppError> {
        self.create_user_events_table().await?;
        self.create_knowledge_base_documents_table().await?;
        self.create_execution_messages_table().await?;
        self.create_memories_table().await?;
        Ok(())
    }

    async fn create_user_events_table(&self) -> Result<(), AppError> {
        let query = r#"
            CREATE TABLE IF NOT EXISTS user_events (
                deployment_id Int64,
                user_id Nullable(Int64),
                event_type String,
                user_name Nullable(String),
                user_email Nullable(String),
                auth_method Nullable(String),
                timestamp DateTime64(3, 'UTC'),
                ip_address Nullable(String),
                INDEX idx_event_type event_type TYPE bloom_filter GRANULARITY 1,
                INDEX idx_user_id user_id TYPE bloom_filter GRANULARITY 1
            ) ENGINE = MergeTree()
            ORDER BY (deployment_id, event_type, timestamp)
            PARTITION BY toYYYYMM(timestamp)
        "#;

        self.client.query(query).execute().await?;
        Ok(())
    }

    async fn create_knowledge_base_documents_table(&self) -> Result<(), AppError> {
        let query = r#"
            CREATE TABLE IF NOT EXISTS knowledge_base_documents (
                id Int64,
                deployment_id Int64,
                knowledge_base_id Int64,
                chunk_index Int32,
                content String,
                embedding Array(Float32) CODEC(NONE),
                created_at DateTime64(3, 'UTC'),
                updated_at DateTime64(3, 'UTC'),
                INDEX idx_kb_embedding embedding TYPE vector_similarity('hnsw', 'L2Distance', 768) GRANULARITY 100000000
            ) ENGINE = MergeTree()
            ORDER BY (knowledge_base_id, chunk_index, id)
            PARTITION BY knowledge_base_id
        "#;

        self.client.query(query).execute().await?;
        Ok(())
    }

    async fn create_execution_messages_table(&self) -> Result<(), AppError> {
        let query = r#"
            CREATE TABLE IF NOT EXISTS execution_messages (
                id Int64,
                deployment_id Int64,
                execution_context_id Int64,
                agent_id Int64,
                message_type String,
                content String,
                embedding Array(Float32) CODEC(NONE),
                created_at DateTime64(3, 'UTC'),
                INDEX idx_msg_embedding embedding TYPE vector_similarity('hnsw', 'L2Distance', 768) GRANULARITY 100000000
            ) ENGINE = MergeTree()
            ORDER BY (execution_context_id, created_at, id)
            PARTITION BY execution_context_id
        "#;

        self.client.query(query).execute().await?;
        Ok(())
    }

    async fn create_memories_table(&self) -> Result<(), AppError> {
        let query = r#"
            CREATE TABLE IF NOT EXISTS memories (
                id Int64,
                deployment_id Int64,
                agent_id Int64,
                execution_context_id Nullable(Int64),
                memory_type String,
                content String,
                embedding Array(Float32) CODEC(NONE),
                importance Float32,
                access_count Int32,
                created_at DateTime64(3, 'UTC'),
                last_accessed_at DateTime64(3, 'UTC'),
                INDEX idx_memory_embedding embedding TYPE vector_similarity('hnsw', 'L2Distance', 768) GRANULARITY 100000000
            ) ENGINE = MergeTree()
            ORDER BY (agent_id, memory_type, importance DESC, created_at, id)
            PARTITION BY agent_id
        "#;

        self.client.query(query).execute().await?;
        Ok(())
    }

    pub async fn insert_user_event(&self, event: &UserEvent) -> Result<(), AppError> {
        let mut insert = self.client.insert("user_events")?;
        insert.write(event).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_total_signups(&self, deployment_id: i64) -> Result<i64, AppError> {
        let query = "SELECT count(DISTINCT user_id) as count FROM user_events WHERE deployment_id = ? AND event_type = 'signup' AND user_id IS NOT NULL";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .fetch_one::<CountResult>()
            .await?;

        Ok(result.count)
    }

    pub async fn get_unique_signins(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let query = "SELECT count(DISTINCT user_id) as count FROM user_events WHERE deployment_id = ? AND event_type = 'signin' AND timestamp >= ? AND timestamp <= ? AND user_id IS NOT NULL";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_one::<CountResult>()
            .await?;

        Ok(result.count)
    }

    pub async fn get_signups(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let query = "SELECT count(*) as count FROM user_events WHERE deployment_id = ? AND event_type = 'signup' AND timestamp >= ? AND timestamp <= ?";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_one::<CountResult>()
            .await?;

        Ok(result.count)
    }

    pub async fn get_organizations_created(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let query = "SELECT count(*) as count FROM user_events WHERE deployment_id = ? AND event_type = 'organization_created' AND timestamp >= ? AND timestamp <= ?";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_one::<CountResult>()
            .await?;

        Ok(result.count)
    }

    pub async fn get_workspaces_created(
        &self,
        deployment_id: i64,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<i64, AppError> {
        let query = "SELECT count(*) as count FROM user_events WHERE deployment_id = ? AND event_type = 'workspace_created' AND timestamp >= ? AND timestamp <= ?";

        let result = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(from.format("%Y-%m-%d %H:%M:%S").to_string())
            .bind(to.format("%Y-%m-%d %H:%M:%S").to_string())
            .fetch_one::<CountResult>()
            .await?;

        Ok(result.count)
    }

    pub async fn get_recent_signups(
        &self,
        deployment_id: i64,
        limit: i32,
    ) -> Result<Vec<RecentSignup>, AppError> {
        let query = "SELECT user_name, user_email, auth_method, timestamp FROM user_events WHERE deployment_id = ? AND event_type = 'signup' ORDER BY timestamp DESC LIMIT ?";

        let rows = self
            .client
            .query(query)
            .bind(deployment_id)
            .bind(limit)
            .fetch_all::<RecentSignupRow>()
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| RecentSignup {
                name: row.user_name,
                email: row.user_email,
                method: row.auth_method,
                date: row.timestamp,
            })
            .collect())
    }

    pub async fn store_knowledge_base_document(
        &self,
        id: i64,
        deployment_id: i64,
        knowledge_base_id: i64,
        chunk_index: i32,
        content: &str,
        embedding: Vec<f32>,
    ) -> Result<(), AppError> {
        let now = Utc::now();

        let doc = KnowledgeBaseDocument {
            id,
            deployment_id,
            knowledge_base_id,
            chunk_index,
            content: content.to_string(),
            embedding,
            created_at: now,
            updated_at: now,
        };

        let mut insert = self.client.insert("knowledge_base_documents")?;
        insert.write(&doc).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn store_execution_message(
        &self,
        id: i64,
        deployment_id: i64,
        execution_context_id: i64,
        agent_id: i64,
        message_type: &str,
        content: &str,
        embedding: Vec<f32>,
    ) -> Result<(), AppError> {
        let now = Utc::now();

        let message = ExecutionMessage {
            id,
            deployment_id,
            execution_context_id,
            agent_id,
            message_type: message_type.to_string(),
            content: content.to_string(),
            embedding,
            created_at: now,
        };

        let mut insert = self.client.insert("execution_messages")?;
        insert.write(&message).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn store_memory(
        &self,
        id: i64,
        deployment_id: i64,
        agent_id: i64,
        execution_context_id: i64,
        memory_type: &str,
        content: &str,
        embedding: Vec<f32>,
        importance: f32,
        access_count: i32,
    ) -> Result<(), AppError> {
        let now = Utc::now();

        let memory = Memory {
            id,
            deployment_id,
            agent_id,
            execution_context_id,
            memory_type: memory_type.to_string(),
            content: content.to_string(),
            embedding,
            importance,
            access_count,
            created_at: now,
            last_accessed_at: now,
        };

        let mut insert = self.client.insert("memories")?;
        insert.write(&memory).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn search_knowledge_base_documents(
        &self,
        knowledge_base_id: i64,
        query_embedding: Vec<f32>,
        limit: u64,
    ) -> Result<Vec<DocumentSearchResult>, AppError> {
        let query = format!(
            "WITH {} AS reference_vector
             SELECT id, content, L2Distance(embedding, reference_vector) as score,
                    knowledge_base_id, chunk_index
             FROM knowledge_base_documents
             WHERE knowledge_base_id = {}
             ORDER BY score ASC LIMIT {}",
            format!(
                "[{}]",
                query_embedding
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            knowledge_base_id,
            limit
        );

        #[derive(Row, Deserialize)]
        struct DocumentSearchRow {
            id: i64,
            content: String,
            score: f32,
            knowledge_base_id: i64,
            chunk_index: i32,
        }

        let rows = self
            .client
            .query(&query)
            .fetch_all::<DocumentSearchRow>()
            .await?;

        let results = rows
            .into_iter()
            .map(|row| DocumentSearchResult {
                id: row.id,
                content: row.content,
                score: row.score,
                knowledge_base_id: row.knowledge_base_id,
                chunk_index: row.chunk_index,
            })
            .collect();

        Ok(results)
    }

    pub async fn search_execution_messages(
        &self,
        execution_context_id: i64,
        query_embedding: Vec<f32>,
        limit: u64,
    ) -> Result<Vec<MessageSearchResult>, AppError> {
        let query = format!(
            "WITH {} AS reference_vector
             SELECT id, content, L2Distance(embedding, reference_vector) as score,
                    execution_context_id, agent_id, message_type
             FROM execution_messages
             WHERE execution_context_id = {}
             ORDER BY score ASC LIMIT {}",
            format!(
                "[{}]",
                query_embedding
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            execution_context_id,
            limit
        );

        #[derive(Row, Deserialize)]
        struct MessageSearchRow {
            id: i64,
            content: String,
            score: f32,
            execution_context_id: i64,
            agent_id: i64,
            message_type: String,
        }

        let rows = self
            .client
            .query(&query)
            .fetch_all::<MessageSearchRow>()
            .await?;

        let results = rows
            .into_iter()
            .map(|row| MessageSearchResult {
                id: row.id,
                content: row.content,
                score: row.score,
                execution_context_id: row.execution_context_id,
                agent_id: row.agent_id,
                message_type: row.message_type,
            })
            .collect();

        Ok(results)
    }

    pub async fn search_memories(
        &self,
        agent_id: i64,
        query_embedding: Vec<f32>,
        limit: u64,
        memory_type_filter: Option<&str>,
    ) -> Result<Vec<MemorySearchResult>, AppError> {
        let mut query = format!(
            "WITH {} AS reference_vector
             SELECT id, content, L2Distance(embedding, reference_vector) as score,
                    agent_id, memory_type, importance, access_count
             FROM memories
             WHERE agent_id = {}",
            format!(
                "[{}]",
                query_embedding
                    .iter()
                    .map(|f| f.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            agent_id
        );

        if let Some(mem_type) = memory_type_filter {
            query.push_str(&format!(" AND memory_type = '{}'", mem_type));
        }

        query.push_str(&format!(" ORDER BY score ASC LIMIT {}", limit));

        #[derive(Row, Deserialize)]
        struct MemorySearchRow {
            id: i64,
            content: String,
            score: f32,
            agent_id: i64,
            memory_type: String,
            importance: f32,
            access_count: i32,
        }

        let rows = self
            .client
            .query(&query)
            .fetch_all::<MemorySearchRow>()
            .await?;

        let results = rows
            .into_iter()
            .map(|row| MemorySearchResult {
                id: row.id,
                content: row.content,
                score: row.score,
                agent_id: row.agent_id,
                memory_type: row.memory_type,
                importance: row.importance,
                access_count: row.access_count,
            })
            .collect();

        Ok(results)
    }

    // Deletion methods for embeddings

    /// Delete all knowledge base document embeddings for a specific knowledge base
    pub async fn delete_knowledge_base_embeddings(
        &self,
        knowledge_base_id: i64,
    ) -> Result<(), AppError> {
        let query = "DELETE FROM knowledge_base_documents WHERE knowledge_base_id = ?";

        self.client
            .query(query)
            .bind(knowledge_base_id)
            .execute()
            .await?;

        Ok(())
    }

    pub async fn delete_document_embeddings(&self, document_id: i64) -> Result<(), AppError> {
        let query = "DELETE FROM knowledge_base_documents WHERE id = ?";

        self.client.query(query).bind(document_id).execute().await?;

        Ok(())
    }

    pub async fn delete_execution_context_embeddings(
        &self,
        execution_context_id: i64,
    ) -> Result<(), AppError> {
        let query = "DELETE FROM execution_messages WHERE execution_context_id = ?";

        self.client
            .query(query)
            .bind(execution_context_id)
            .execute()
            .await?;

        Ok(())
    }

    /// Delete memory embeddings for a specific agent
    pub async fn delete_agent_memories(&self, agent_id: i64) -> Result<(), AppError> {
        let query = "DELETE FROM memories WHERE agent_id = ?";

        self.client.query(query).bind(agent_id).execute().await?;

        Ok(())
    }

    /// Delete memory embeddings for a specific execution context
    pub async fn delete_execution_context_memories(
        &self,
        execution_context_id: i64,
    ) -> Result<(), AppError> {
        let query = "DELETE FROM memories WHERE execution_context_id = ?";

        self.client
            .query(query)
            .bind(execution_context_id)
            .execute()
            .await?;

        Ok(())
    }
}
