use crate::{
    commands::Command,
    error::AppError,
    models::{ConversationRecord, MemoryRecordV2, ConversationContent, ConversationMessageType},
    state::AppState,
};
use chrono::Utc;
use pgvector::Vector;

pub struct CreateConversationCommand {
    pub id: i64,
    pub context_id: i64,
    pub content: ConversationContent,  // Changed to typed content
    pub message_type: ConversationMessageType,
}

impl CreateConversationCommand {
    pub fn new(id: i64, context_id: i64, content: ConversationContent, message_type: ConversationMessageType) -> Self {
        Self {
            id,
            context_id,
            content,
            message_type,
        }
    }
}

impl Command for CreateConversationCommand {
    type Output = ConversationRecord;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        // Convert typed content to JSON for database storage
        let content_json = serde_json::to_value(&self.content)
            .map_err(|e| AppError::Internal(format!("Failed to serialize content: {}", e)))?;
        
        // Convert enum to string for database storage  
        let message_type_str = match self.message_type {
            ConversationMessageType::UserMessage => "user_message",
            ConversationMessageType::AgentResponse => "agent_response", 
            ConversationMessageType::AssistantAcknowledgment => "assistant_acknowledgment",
            ConversationMessageType::AssistantIdeation => "assistant_ideation",
            ConversationMessageType::AssistantTaskExecution => "assistant_task_execution",
            ConversationMessageType::AssistantValidation => "assistant_validation",
        };

        let record = sqlx::query_as::<_, ConversationRecord>(
            r#"
            INSERT INTO conversations (
                id, context_id, timestamp, content, message_type,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $6
            )
            RETURNING *
            "#,
        )
        .bind(self.id)
        .bind(self.context_id)
        .bind(now)
        .bind(content_json)
        .bind(message_type_str)
        .bind(now)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(record)
    }
}

/// Command to create a new memory record
pub struct CreateMemoryV2Command {
    pub id: i64,
    pub content: String,
    pub embedding: Vec<f32>,
    pub memory_category: String, // "procedural", "semantic", "episodic"
    pub creation_context_id: Option<i64>,
    pub initial_importance: f64,
}

impl Command for CreateMemoryV2Command {
    type Output = MemoryRecordV2;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let embedding = if self.embedding.is_empty() {
            None
        } else {
            Some(Vector::from(self.embedding))
        };

        let record = sqlx::query_as::<_, MemoryRecordV2>(
            r#"
            INSERT INTO memories (
                id, content, embedding, memory_category,
                base_temporal_score, access_count, first_accessed_at, last_accessed_at,
                citation_count, cross_context_value, learning_confidence,
                creation_context_id, last_reinforced_at,
                semantic_centrality, uniqueness_score,
                compression_level, compressed_content,
                context_decay_profile,
                created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4,
                $5, 0, $6, $6,
                0, 0.0, 0.5,
                $7, $6,
                0.0, 0.0,
                0, NULL,
                '{}',
                $6, $6
            )
            RETURNING *
            "#,
        )
        .bind(self.id)
        .bind(self.content)
        .bind(embedding)
        .bind(self.memory_category)
        .bind(self.initial_importance)
        .bind(now)
        .bind(self.creation_context_id)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(record)
    }
}

/// Command to update citation metrics after LLM usage
pub struct UpdateCitationMetricsCommand {
    pub item_id: i64,
    pub item_type: CitationType,
    pub relevance_delta: f64,
    pub usefulness_delta: f64,
}

pub enum CitationType {
    Memory,
}

impl Command for UpdateCitationMetricsCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        match self.item_type {
            CitationType::Memory => {
                sqlx::query(
                    r#"
                    UPDATE memories
                    SET citation_count = citation_count + 1,
                        relevance_score = LEAST(relevance_score + $2, 1.0),
                        usefulness_score = LEAST(usefulness_score + $3, 1.0),
                        last_reinforced_at = NOW(),
                        base_temporal_score = calculate_base_decay(
                            access_count,
                            citation_count + 1,
                            first_accessed_at,
                            last_accessed_at,
                            LEAST(relevance_score + $2, 1.0),
                            LEAST(usefulness_score + $3, 1.0),
                            compression_level
                        )
                    WHERE id = $1
                    "#,
                )
                .bind(self.item_id)
                .bind(self.relevance_delta)
                .bind(self.usefulness_delta)
                .execute(&app_state.db_pool)
                .await?;
            }
        }

        Ok(())
    }
}

/// Command to batch create multiple conversations (for migration)
pub struct BatchCreateConversationsCommand {
    pub conversations: Vec<CreateConversationCommand>,
}

impl Command for BatchCreateConversationsCommand {
    type Output = Vec<ConversationRecord>;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut results = Vec::new();

        // Use a transaction for consistency
        let tx = app_state.db_pool.begin().await?;

        for conv in self.conversations {
            let record = conv.execute(app_state).await?;
            results.push(record);
        }

        tx.commit().await?;

        Ok(results)
    }
}

/// Store memory asynchronously (fire-and-forget)
pub fn store_memory_async(
    _app_state: AppState,
    _content: String,
    memory_category: String,
    context_id: Option<i64>,
    importance: f64,
) {
    // TODO: Implement async processing
    // The calling code should spawn this task using tokio::spawn
    // For now, just log that we would store this memory
    tracing::info!(
        "TODO: Would store {} memory with importance {} for context {:?}",
        memory_category,
        importance,
        context_id
    );
}
