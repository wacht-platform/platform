use crate::Command;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use models::{AgentExecutionContext, AgentExecutionState, ExecutionContextStatus};

pub struct CreateExecutionContextCommand {
    pub deployment_id: i64,
    pub title: Option<String>,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
}

impl CreateExecutionContextCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            title: None,
            system_instructions: None,
            context_group: None,
        }
    }

    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    pub fn with_system_instructions(mut self, system_instructions: String) -> Self {
        self.system_instructions = Some(system_instructions);
        self
    }

    pub fn with_context_group(mut self, context_group: String) -> Self {
        self.context_group = Some(context_group);
        self
    }
}

impl Command for CreateExecutionContextCommand {
    type Output = AgentExecutionContext;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let context_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO agent_execution_contexts
            (id, created_at, updated_at, deployment_id, title, system_instructions, context_group, last_activity_at, status)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            context_id,
            now,
            now,
            self.deployment_id,
            self.title.as_deref().unwrap_or(""),
            self.system_instructions,
            self.context_group,
            now,
            "idle"
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AgentExecutionContext {
            id: context_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            title: self.title.unwrap_or_default(),
            system_instructions: self.system_instructions,
            context_group: self.context_group,
            last_activity_at: now,
            completed_at: None,
            execution_state: None,
            status: ExecutionContextStatus::Idle,
            source: None,
            external_context_id: None,
            external_resource_metadata: None,
        })
    }
}

pub struct UpdateExecutionContextQuery {
    pub context_id: i64,
    pub deployment_id: i64,
    pub system_instructions: Option<String>,
    pub completed_at: Option<Option<DateTime<Utc>>>,
    pub execution_state: Option<AgentExecutionState>,
    pub status: Option<ExecutionContextStatus>,
}

impl UpdateExecutionContextQuery {
    pub fn new(context_id: i64, deployment_id: i64) -> Self {
        Self {
            context_id,
            deployment_id,
            system_instructions: None,
            completed_at: None,
            execution_state: None,
            status: None,
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

    pub fn with_execution_state(mut self, state: AgentExecutionState) -> Self {
        self.execution_state = Some(state);
        self
    }

    pub fn with_status(mut self, status: ExecutionContextStatus) -> Self {
        self.status = Some(status);
        self
    }
}

impl super::Command for UpdateExecutionContextQuery {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();

        if let Some(ref system_instructions) = self.system_instructions {
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, system_instructions = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                system_instructions,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(completed_at) = self.completed_at {
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, completed_at = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                completed_at,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(ref state) = self.execution_state {
            let state_json = serde_json::to_value(state)?;
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, execution_state = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                state_json,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        if let Some(ref status) = self.status {
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, status = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                status.to_string(),
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

            // If status is being set to Failed, it's likely a cancellation - log it in conversation
            if matches!(status, ExecutionContextStatus::Failed) {
                use crate::CreateConversationCommand;
                use models::{ConversationContent, ConversationMessageType};

                let cancel_command = CreateConversationCommand::new(
                    app_state.sf.next_id()? as i64,
                    self.context_id,
                    ConversationContent::SystemDecision {
                        step: "execution_cancelled".to_string(),
                        reasoning: "User requested cancellation of the current execution"
                            .to_string(),
                        confidence: 1.0,
                        thought_signature: None,
                    },
                    ConversationMessageType::SystemDecision,
                );

                // Execute the command to store the cancellation message
                if let Ok(conversation) = cancel_command.execute(app_state).await {
                    // Publish the cancellation message to the stream
                    let subject = format!("agent_execution_stream.context:{}", self.context_id);
                    let mut headers = async_nats::HeaderMap::new();
                    headers.insert("message_type", "conversation_message");

                    if let Ok(payload) = serde_json::to_vec(&conversation) {
                        let _ = app_state
                            .nats_jetstream
                            .publish_with_headers(subject, headers, payload.into())
                            .await;
                    }
                }
            }
        }

        Ok(())
    }
}

pub struct UpdateExecutionContextCommand {
    pub context_id: i64,
    pub deployment_id: i64,
    pub title: Option<String>,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
    pub status: Option<ExecutionContextStatus>,
}

impl UpdateExecutionContextCommand {
    pub fn new(context_id: i64, deployment_id: i64) -> Self {
        Self {
            context_id,
            deployment_id,
            title: None,
            system_instructions: None,
            context_group: None,
            status: None,
        }
    }

    pub fn with_title(mut self, title: String) -> Self {
        self.title = Some(title);
        self
    }

    pub fn with_system_instructions(mut self, system_instructions: String) -> Self {
        self.system_instructions = Some(system_instructions);
        self
    }

    pub fn with_context_group(mut self, context_group: String) -> Self {
        self.context_group = Some(context_group);
        self
    }

    pub fn with_status(mut self, status: ExecutionContextStatus) -> Self {
        self.status = Some(status);
        self
    }
}

impl Command for UpdateExecutionContextCommand {
    type Output = AgentExecutionContext;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let now = Utc::now();
        sqlx::query!(
            r#"
            UPDATE agent_execution_contexts
            SET
                updated_at = $1,
                last_activity_at = $2,
                title = COALESCE($3, title),
                system_instructions = COALESCE($4, system_instructions),
                context_group = COALESCE($5, context_group),
                status = COALESCE($6, status)
            WHERE id = $7 AND deployment_id = $8
            "#,
            now,
            now,
            self.title.as_deref(),
            self.system_instructions.as_deref(),
            self.context_group.as_deref(),
            self.status.as_ref().map(|s| s.to_string()),
            self.context_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Fetch and return the updated context
        use queries::{GetExecutionContextQuery, Query as QueryTrait};
        GetExecutionContextQuery::new(self.context_id, self.deployment_id)
            .execute(app_state)
            .await
    }
}
