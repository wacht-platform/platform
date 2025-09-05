use crate::Command;
use common::error::AppError;
use common::state::AppState;
use models::{ConversationContent, ConversationMessageType, ConversationRecord};

use chrono::Utc;
use tiktoken_rs::cl100k_base;

pub struct CreateConversationCommand {
    pub id: i64,
    pub context_id: i64,
    pub content: ConversationContent,
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

    fn calculate_token_count(&self) -> Result<i32, AppError> {
        let text = match &self.content {
            ConversationContent::UserMessage { message, .. } => message.clone(),
            ConversationContent::AgentResponse { response, .. } => response.clone(),
            ConversationContent::AssistantAcknowledgment {
                acknowledgment_message,
                reasoning,
                ..
            } => {
                format!("{} {}", acknowledgment_message, reasoning)
            }
            ConversationContent::AssistantIdeation {
                reasoning_summary, ..
            } => reasoning_summary.clone(),
            ConversationContent::ExecutionSummary { token_count, .. } => {
                // For execution summaries, use the pre-calculated token count
                return Ok(*token_count as i32);
            }
            ConversationContent::PlatformFunctionResult {
                execution_id,
                result,
            } => {
                format!(
                    "Platform function execution {} result: {}",
                    execution_id, result
                )
            }
            _ => {
                // For other complex types, serialize to JSON and count
                serde_json::to_string(&self.content).unwrap_or_else(|_| "{}".to_string())
            }
        };

        let bpe = cl100k_base()
            .map_err(|e| AppError::Internal(format!("Failed to init tokenizer: {}", e)))?;
        let tokens = bpe.encode_with_special_tokens(&text);

        Ok(tokens.len() as i32)
    }
}

impl Command for CreateConversationCommand {
    type Output = ConversationRecord;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        // Convert typed content to JSON for database storage
        let content_json = serde_json::to_value(&self.content)
            .map_err(|e| AppError::Internal(format!("Failed to serialize content: {}", e)))?;

        // Calculate token count
        let token_count = self.calculate_token_count()?;

        // Convert enum to string for database storage
        let message_type_str = match self.message_type {
            ConversationMessageType::UserMessage => "user_message",
            ConversationMessageType::AgentResponse => "agent_response",
            ConversationMessageType::AssistantAcknowledgment => "assistant_acknowledgment",
            ConversationMessageType::AssistantIdeation => "assistant_ideation",
            ConversationMessageType::AssistantActionPlanning => "assistant_action_planning",
            ConversationMessageType::ActionExecutionResult => "action_execution_result",
            ConversationMessageType::AssistantValidation => "assistant_validation",
            ConversationMessageType::SystemDecision => "system_decision",
            ConversationMessageType::ContextResults => "context_results",
            ConversationMessageType::UserInputRequest => "user_input_request",
            ConversationMessageType::ExecutionSummary => "execution_summary",
            ConversationMessageType::PlatformFunctionResult => "platform_function_result",
        };

        let record = sqlx::query_as::<_, ConversationRecord>(
            r#"
            INSERT INTO conversations (
                id, context_id, timestamp, content, message_type,
                token_count, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $7
            )
            RETURNING *
            "#,
        )
        .bind(self.id)
        .bind(self.context_id)
        .bind(now)
        .bind(content_json)
        .bind(message_type_str)
        .bind(token_count)
        .bind(now)
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(AppError::from)?;

        Ok(record)
    }
}
