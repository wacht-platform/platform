use chrono::{DateTime, Utc};
use common::{HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, error::AppError};
use models::{AgentThreadStatus, ThreadExecutionState};

pub struct UpdateAgentThreadStateCommand {
    pub thread_id: i64,
    pub deployment_id: i64,
    pub system_instructions: Option<String>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
    pub execution_state: Option<ThreadExecutionState>,
    pub status: Option<AgentThreadStatus>,
    pub status_marked_as_cancellation: bool,
}

impl UpdateAgentThreadStateCommand {
    pub fn new(thread_id: i64, deployment_id: i64) -> Self {
        Self {
            thread_id,
            deployment_id,
            system_instructions: None,
            completed_at: None,
            execution_state: None,
            status: None,
            status_marked_as_cancellation: false,
        }
    }

    pub fn with_system_instructions(mut self, system_instructions: String) -> Self {
        self.system_instructions = Some(system_instructions);
        self
    }

    pub fn with_completion(mut self) -> Self {
        self.completed_at = Some(Some(Utc::now()));
        self
    }

    pub fn with_execution_state(mut self, state: ThreadExecutionState) -> Self {
        self.execution_state = Some(state);
        self
    }

    pub fn with_status(mut self, status: AgentThreadStatus) -> Self {
        self.status = Some(status);
        self
    }

    pub fn mark_status_as_cancellation(mut self) -> Self {
        self.status_marked_as_cancellation = true;
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<(), AppError>
    where
        D: HasDbRouter + HasNatsJetStreamProvider + HasIdProvider,
    {
        let now = Utc::now();
        let cancellation_message_id = deps.id_provider().next_id()? as i64;

        if let Some(ref system_instructions) = self.system_instructions {
            sqlx::query!(
                "UPDATE agent_threads SET updated_at = $1, last_activity_at = $1, system_instructions = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                system_instructions,
                self.thread_id,
                self.deployment_id
            )
            .execute(deps.writer_pool())
            .await
            .map_err(AppError::Database)?;
        }

        if let Some(completed_at) = self.completed_at {
            sqlx::query!(
                "UPDATE agent_threads SET updated_at = $1, last_activity_at = $1, completed_at = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                completed_at,
                self.thread_id,
                self.deployment_id
            )
            .execute(deps.writer_pool())
            .await
            .map_err(AppError::Database)?;
        }

        if let Some(ref state) = self.execution_state {
            let state_json = serde_json::to_value(state)?;
            sqlx::query!(
                "UPDATE agent_threads SET updated_at = $1, last_activity_at = $1, execution_state = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                state_json,
                self.thread_id,
                self.deployment_id
            )
            .execute(deps.writer_pool())
            .await
            .map_err(AppError::Database)?;
        }

        if let Some(ref status) = self.status {
            let thread_status = status.to_string();
            let status_update = sqlx::query!(
                "UPDATE agent_threads SET updated_at = $1, last_activity_at = $1, status = $2 WHERE id = $3 AND deployment_id = $4 AND status IS DISTINCT FROM $2",
                now,
                thread_status,
                self.thread_id,
                self.deployment_id
            )
            .execute(deps.writer_pool())
            .await
            .map_err(|e| AppError::Database(e))?;

            let status_changed = status_update.rows_affected() > 0;

            if status_changed
                && matches!(status, AgentThreadStatus::Failed)
                && self.status_marked_as_cancellation
            {
                use crate::CreateConversationCommand;
                use models::{ConversationContent, ConversationMessageType};

                let cancel_command = CreateConversationCommand::new(
                    cancellation_message_id,
                    self.thread_id,
                    ConversationContent::SystemDecision {
                        step: "execution_cancelled".to_string(),
                        reasoning: "User requested cancellation of the current execution"
                            .to_string(),
                        confidence: 1.0,
                    },
                    ConversationMessageType::SystemDecision,
                );

                if let Ok(conversation) = cancel_command.execute_with_db(deps.writer_pool()).await {
                    let subject = format!("agent_execution_stream.thread:{}", self.thread_id);
                    let mut headers = async_nats::HeaderMap::new();
                    headers.insert("message_type", "conversation_message");

                    if let Ok(payload) = serde_json::to_vec(&conversation) {
                        let _ = deps
                            .nats_jetstream_provider()
                            .publish_with_headers(subject, headers, payload.into())
                            .await;
                    }
                }
            }
        }

        Ok(())
    }
}
