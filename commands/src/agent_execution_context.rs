use crate::Command;
use chrono::{DateTime, Utc};
use common::error::AppError;
use common::state::AppState;
use models::{
    AgentExecutionContext, AgentExecutionState, AgentStatusUpdate, ExecutionContextStatus,
};
use std::collections::VecDeque;

pub struct CreateExecutionContextCommand {
    pub deployment_id: i64,
    pub title: Option<String>,
    pub system_instructions: Option<String>,
    pub context_group: Option<String>,
    pub parent_context_id: Option<i64>,
}

impl CreateExecutionContextCommand {
    pub fn new(deployment_id: i64) -> Self {
        Self {
            deployment_id,
            title: None,
            system_instructions: None,
            context_group: None,
            parent_context_id: None,
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

    pub fn with_parent(mut self, parent_context_id: i64) -> Self {
        self.parent_context_id = Some(parent_context_id);
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
            (id, created_at, updated_at, deployment_id, title, system_instructions, context_group, last_activity_at, status, parent_context_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            context_id,
            now,
            now,
            self.deployment_id,
            self.title.as_deref().unwrap_or(""),
            self.system_instructions,
            self.context_group,
            now,
            "idle",
            self.parent_context_id
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
            parent_context_id: self.parent_context_id,
            completion_summary: None,
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
    pub external_resource_metadata: Option<serde_json::Value>,
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
            external_resource_metadata: None,
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

    pub fn with_external_resource_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.external_resource_metadata = Some(metadata);
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
            let status_update = sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, status = $2 WHERE id = $3 AND deployment_id = $4 AND status IS DISTINCT FROM $2",
                now,
                status.to_string(),
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;

            let status_changed = status_update.rows_affected() > 0;

            // If status is being set to Failed, it's likely a cancellation - log it in conversation
            if status_changed && matches!(status, ExecutionContextStatus::Failed) {
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

            if status_changed
                && matches!(
                    status,
                    ExecutionContextStatus::Failed | ExecutionContextStatus::Interrupted
                )
            {
                CancelDescendantExecutionsCommand::new(self.context_id, self.deployment_id)
                    .execute(app_state)
                    .await?;
            }
        }

        if let Some(ref metadata) = self.external_resource_metadata {
            sqlx::query!(
                "UPDATE agent_execution_contexts SET updated_at = $1, last_activity_at = $1, external_resource_metadata = $2 WHERE id = $3 AND deployment_id = $4",
                now,
                metadata,
                self.context_id,
                self.deployment_id
            )
            .execute(&app_state.db_pool)
            .await
            .map_err(|e| AppError::Database(e))?;
        }

        Ok(())
    }
}

/// Cancel all descendant contexts for an aborted parent context.
/// This is event-driven: marks descendants as cancelled and publishes spawn-control stop events.
pub struct CancelDescendantExecutionsCommand {
    pub parent_context_id: i64,
    pub deployment_id: i64,
}

impl CancelDescendantExecutionsCommand {
    pub fn new(parent_context_id: i64, deployment_id: i64) -> Self {
        Self {
            parent_context_id,
            deployment_id,
        }
    }
}

impl Command for CancelDescendantExecutionsCommand {
    type Output = usize;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let mut cancelled_count = 0usize;
        let mut queue = VecDeque::from([self.parent_context_id]);
        let cancelled_summary = serde_json::json!({
            "status": "Cancelled",
            "result": null,
            "error_message": format!("Cancelled because ancestor context {} was aborted.", self.parent_context_id),
            "metrics": null
        });

        while let Some(current_parent_id) = queue.pop_front() {
            let child_ids = sqlx::query_scalar!(
                r#"
                SELECT id
                FROM agent_execution_contexts
                WHERE parent_context_id = $1 AND deployment_id = $2
                "#,
                current_parent_id,
                self.deployment_id
            )
            .fetch_all(&app_state.db_pool)
            .await
            .map_err(AppError::Database)?;

            for child_id in child_ids {
                queue.push_back(child_id);

                sqlx::query!(
                    r#"
                    UPDATE agent_execution_contexts
                    SET status = 'failed',
                        completion_summary = COALESCE(completion_summary, $1),
                        completed_at = COALESCE(completed_at, NOW()),
                        updated_at = NOW(),
                        last_activity_at = NOW()
                    WHERE id = $2
                      AND deployment_id = $3
                      AND status NOT IN ('completed', 'failed')
                    "#,
                    cancelled_summary.clone(),
                    child_id,
                    self.deployment_id
                )
                .execute(&app_state.db_pool)
                .await
                .map_err(AppError::Database)?;

                let _ = PublishSpawnControlCommand::new(
                    child_id,
                    self.deployment_id,
                    SpawnControlAction::Stop,
                )
                .execute(app_state)
                .await;
                cancelled_count += 1;
            }
        }

        Ok(cancelled_count)
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

// ============================================================================
// Multi-Agent Support Commands
// ============================================================================

/// Post a status update to an agent's execution timeline
pub struct PostStatusUpdateCommand {
    pub context_id: i64,
    pub deployment_id: i64,
    pub status_update: String,
    pub metadata: Option<serde_json::Value>,
}

impl PostStatusUpdateCommand {
    pub fn new(context_id: i64, deployment_id: i64, status_update: String) -> Self {
        Self {
            context_id,
            deployment_id,
            status_update,
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl Command for PostStatusUpdateCommand {
    type Output = AgentStatusUpdate;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let id = app_state.sf.next_id()? as i64;

        let row = sqlx::query!(
            "INSERT INTO agent_status_updates (id, context_id, status_update, metadata, created_at)
             VALUES ($1, $2, $3, $4, NOW())
             RETURNING id, context_id, status_update, metadata, created_at",
            id,
            self.context_id,
            self.status_update,
            self.metadata
        )
        .fetch_one(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        // Update last_activity_at on context
        sqlx::query!(
            "UPDATE agent_execution_contexts SET last_activity_at = NOW() WHERE id = $1",
            self.context_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AgentStatusUpdate {
            id: row.id,
            context_id: row.context_id,
            status_update: row.status_update,
            metadata: row.metadata,
            created_at: row.created_at.unwrap_or_else(|| chrono::Utc::now()),
        })
    }
}

/// Create a child execution context (spawned by a parent agent)
pub struct CreateChildContextCommand {
    pub deployment_id: i64,
    pub parent_context_id: i64,
    pub title: String,
    pub initial_task: String,
    pub task_type: String,
}

impl CreateChildContextCommand {
    pub fn new(deployment_id: i64, parent_context_id: i64, title: String) -> Self {
        Self {
            deployment_id,
            parent_context_id,
            title,
            initial_task: String::new(),
            task_type: "delegate".to_string(),
        }
    }

    pub fn with_initial_task(mut self, task: String) -> Self {
        self.initial_task = task;
        self
    }

    pub fn with_task_type(mut self, task_type: String) -> Self {
        self.task_type = task_type;
        self
    }
}

impl Command for CreateChildContextCommand {
    type Output = AgentExecutionContext;

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let context_id = app_state.sf.next_id()? as i64;
        let now = Utc::now();

        sqlx::query!(
            r#"
            INSERT INTO agent_execution_contexts
            (id, created_at, updated_at, deployment_id, title, system_instructions, context_group, last_activity_at, status, parent_context_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            "#,
            context_id,
            now,
            now,
            self.deployment_id,
            self.title,
            Option::<String>::None,
            Option::<String>::None,
            now,
            "idle",
            self.parent_context_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(AgentExecutionContext {
            id: context_id,
            created_at: now,
            updated_at: now,
            deployment_id: self.deployment_id,
            title: self.title,
            system_instructions: None,
            context_group: None,
            last_activity_at: now,
            completed_at: None,
            execution_state: None,
            status: ExecutionContextStatus::Idle,
            source: None,
            external_context_id: None,
            external_resource_metadata: None,
            parent_context_id: Some(self.parent_context_id),
            completion_summary: None,
        })
    }
}

// ============================================================================
// Spawn Control Messaging
// ============================================================================

/// Control action to send to a spawned child agent
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SpawnControlAction {
    Stop,
    Restart,
    UpdateParams(serde_json::Value),
}

/// Publish a control message to a spawned child agent
pub struct PublishSpawnControlCommand {
    pub child_context_id: i64,
    pub deployment_id: i64,
    pub action: SpawnControlAction,
    pub sender_context_id: Option<i64>,
}

impl PublishSpawnControlCommand {
    pub fn new(child_context_id: i64, deployment_id: i64, action: SpawnControlAction) -> Self {
        Self {
            child_context_id,
            deployment_id,
            action,
            sender_context_id: None,
        }
    }

    pub fn with_sender(mut self, sender_context_id: i64) -> Self {
        self.sender_context_id = Some(sender_context_id);
        self
    }

    fn subject(&self) -> String {
        format!("agent_spawn_control.context:{}", self.child_context_id)
    }
}

impl Command for PublishSpawnControlCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let (action_type, action_value) = match &self.action {
            SpawnControlAction::Stop => ("stop".to_string(), serde_json::Value::Null),
            SpawnControlAction::Restart => ("restart".to_string(), serde_json::Value::Null),
            SpawnControlAction::UpdateParams(params) => {
                ("update_params".to_string(), params.clone())
            }
        };

        let payload = serde_json::json!({
            "child_context_id": self.child_context_id,
            "deployment_id": self.deployment_id,
            "sender_context_id": self.sender_context_id,
            "action": action_type,
            "value": action_value,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let payload_bytes = serde_json::to_vec(&payload).map_err(|e| {
            AppError::Internal(format!("Failed to serialize control message: {}", e))
        })?;

        app_state
            .nats_jetstream
            .publish(self.subject(), payload_bytes.into())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to publish spawn control: {}", e)))?;

        tracing::info!(
            child_context_id = self.child_context_id,
            action = ?self.action,
            "Published spawn control message"
        );

        Ok(())
    }
}

// ============================================================================
// Enhanced Completion Summary
// ============================================================================

/// Structured completion summary for child agents
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompletionSummary {
    pub status: CompletionStatus,
    pub result: Option<String>,
    pub error_message: Option<String>,
    pub metrics: Option<CompletionMetrics>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum CompletionStatus {
    Success,
    Failed,
    Timeout,
    Cancelled,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompletionMetrics {
    pub steps_taken: Option<u32>,
    pub time_elapsed_secs: Option<u64>,
    pub tools_used: Option<Vec<String>>,
}

/// Enhanced completion summary command
pub struct StoreCompletionSummaryEnhancedCommand {
    pub context_id: i64,
    pub deployment_id: i64,
    pub summary: CompletionSummary,
}

impl StoreCompletionSummaryEnhancedCommand {
    pub fn new(context_id: i64, deployment_id: i64, summary: CompletionSummary) -> Self {
        Self {
            context_id,
            deployment_id,
            summary,
        }
    }
}

impl Command for StoreCompletionSummaryEnhancedCommand {
    type Output = ();

    async fn execute(self, app_state: &AppState) -> Result<Self::Output, AppError> {
        let status = self.summary.status.clone();
        let summary_json = serde_json::to_value(self.summary)
            .map_err(|e| AppError::Internal(format!("Failed to serialize summary: {}", e)))?;

        // Also set status based on completion status
        let status_str = match status {
            CompletionStatus::Success => "completed",
            CompletionStatus::Failed | CompletionStatus::Timeout | CompletionStatus::Cancelled => {
                "failed"
            }
        };

        sqlx::query!(
            r#"
            UPDATE agent_execution_contexts
            SET completion_summary = $1,
                status = $2,
                completed_at = COALESCE(completed_at, NOW()),
                updated_at = NOW(),
                last_activity_at = NOW()
            WHERE id = $3 AND deployment_id = $4
            "#,
            summary_json,
            status_str,
            self.context_id,
            self.deployment_id
        )
        .execute(&app_state.db_pool)
        .await
        .map_err(|e| AppError::Database(e))?;

        tracing::info!(
            context_id = self.context_id,
            status = ?status,
            "Stored completion summary"
        );

        Ok(())
    }
}
