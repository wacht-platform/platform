use crate::{
    commands::Command,
    error::AppError,
    models::{ConversationContent, ConversationMessageType, ConversationRecord, MemoryRecord},
    state::AppState,
};
use chrono::Utc;
use pgvector::Vector;

pub struct CreateConversationCommand {
    pub id: i64,
    pub context_id: i64,
    pub content: ConversationContent, // Changed to typed content
    pub message_type: ConversationMessageType,
}

impl CreateConversationCommand {
    pub fn new(
        id: i64,
        context_id: i64,
        content: ConversationContent,
        message_type: ConversationMessageType,
    ) -> Self {
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
            ConversationMessageType::AssistantActionPlanning => "assistant_action_planning",
            ConversationMessageType::AssistantTaskExecution => "assistant_task_execution",
            ConversationMessageType::AssistantValidation => "assistant_validation",
            ConversationMessageType::SystemDecision => "system_decision",
            ConversationMessageType::ContextResults => "context_results",
            ConversationMessageType::UserInputRequest => "user_input_request",
            ConversationMessageType::ExecutionSummary => "execution_summary",
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
pub struct CreateMemoryCommand {
    pub id: i64,
    pub content: String,
    pub embedding: Vec<f32>,
    pub memory_category: String, // "procedural", "semantic", "episodic"
    pub creation_context_id: Option<i64>,
    pub initial_importance: f64,
}

impl Command for CreateMemoryCommand {
    type Output = MemoryRecord;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        let embedding = if self.embedding.is_empty() {
            None
        } else {
            Some(Vector::from(self.embedding))
        };

        let record = sqlx::query_as::<_, MemoryRecord>(
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
