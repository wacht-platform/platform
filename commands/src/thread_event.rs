use chrono::Utc;
use common::{
    HasDbRouter, HasIdProvider, HasNatsJetStreamProvider, ReadConsistency, error::AppError,
};
use models::{AgentThreadStatus, ThreadEvent};
use queries::GetAgentThreadStateQuery;

use crate::{PublishThreadScheduleCommand, UpsertThreadAgentAssignmentCommand};

pub struct EnqueueThreadEventCommand {
    pub id: i64,
    pub deployment_id: i64,
    pub thread_id: i64,
    pub board_item_id: Option<i64>,
    pub event_type: String,
    pub status: String,
    pub priority: i32,
    pub payload: serde_json::Value,
    pub available_at: Option<chrono::DateTime<Utc>>,
    pub caused_by_run_id: Option<i64>,
    pub caused_by_thread_id: Option<i64>,
    pub conversation_id: Option<i64>,
    pub max_retries: i32,
}

pub struct UpdateThreadEventStateCommand {
    pub event_id: i64,
    pub status: String,
    pub caused_by_run_id: Option<i64>,
    pub claimed_at: Option<chrono::DateTime<Utc>>,
    pub completed_at: Option<chrono::DateTime<Utc>>,
    pub failed_at: Option<chrono::DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadEventWakeDisposition {
    Published,
    NoopThreadActive,
    NoopThreadTerminal,
    NoopNoResolvedAgent,
}

pub struct DispatchThreadEventCommand {
    enqueue: EnqueueThreadEventCommand,
    preferred_agent_id: Option<i64>,
}

pub struct DispatchThreadEventResult {
    pub event: ThreadEvent,
    pub wake_disposition: ThreadEventWakeDisposition,
}

pub fn wake_disposition_for_thread_event(
    thread_status: &AgentThreadStatus,
    event_type: &str,
    reusable: bool,
    accepts_assignments: bool,
) -> ThreadEventWakeDisposition {
    let is_orchestration_event = matches!(
        event_type,
        models::thread_event::event_type::TASK_ROUTING
            | models::thread_event::event_type::ASSIGNMENT_EXECUTION
    );
    let recoverable_failed_for_work = is_orchestration_event && (reusable || accepts_assignments);

    match thread_status {
        AgentThreadStatus::Running => {
            if is_orchestration_event {
                ThreadEventWakeDisposition::Published
            } else {
                ThreadEventWakeDisposition::NoopThreadActive
            }
        }
        AgentThreadStatus::WaitingForInput => match event_type {
            models::thread_event::event_type::USER_MESSAGE_RECEIVED
            | models::thread_event::event_type::USER_INPUT_RECEIVED
            | models::thread_event::event_type::APPROVAL_RESPONSE_RECEIVED
            | models::thread_event::event_type::TASK_ROUTING
            | models::thread_event::event_type::ASSIGNMENT_EXECUTION => {
                ThreadEventWakeDisposition::Published
            }
            _ => ThreadEventWakeDisposition::NoopThreadActive,
        },
        AgentThreadStatus::Failed if recoverable_failed_for_work => {
            ThreadEventWakeDisposition::Published
        }
        AgentThreadStatus::Failed => ThreadEventWakeDisposition::NoopThreadTerminal,
        AgentThreadStatus::Idle | AgentThreadStatus::Interrupted | AgentThreadStatus::Completed => {
            ThreadEventWakeDisposition::Published
        }
    }
}

fn default_max_retries_for_event_type(event_type: &str) -> i32 {
    if models::thread_event::event_purpose::is_user_facing(event_type) {
        2
    } else {
        0
    }
}

impl EnqueueThreadEventCommand {
    pub fn new(id: i64, deployment_id: i64, thread_id: i64, event_type: String) -> Self {
        let max_retries = default_max_retries_for_event_type(event_type.as_str());
        Self {
            id,
            deployment_id,
            thread_id,
            board_item_id: None,
            event_type,
            status: models::thread_event::status::PENDING.to_string(),
            priority: 100,
            payload: serde_json::json!({}),
            available_at: None,
            caused_by_run_id: None,
            caused_by_thread_id: None,
            conversation_id: None,
            max_retries,
        }
    }

    pub fn with_max_retries(mut self, max_retries: i32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_board_item_id(mut self, board_item_id: i64) -> Self {
        self.board_item_id = Some(board_item_id);
        self
    }

    pub fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_payload(mut self, payload: serde_json::Value) -> Self {
        self.payload = payload;
        self
    }

    pub fn with_available_at(mut self, available_at: chrono::DateTime<Utc>) -> Self {
        self.available_at = Some(available_at);
        self
    }

    pub fn with_caused_by_run_id(mut self, run_id: i64) -> Self {
        self.caused_by_run_id = Some(run_id);
        self
    }

    pub fn with_caused_by_thread_id(mut self, thread_id: i64) -> Self {
        self.caused_by_thread_id = Some(thread_id);
        self
    }

    pub fn with_conversation_id(mut self, conversation_id: i64) -> Self {
        self.conversation_id = Some(conversation_id);
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ThreadEvent, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        self.validate()?;
        let now = Utc::now();
        let available_at = self.available_at.unwrap_or(now);
        let event = sqlx::query_as!(
            ThreadEvent,
            r#"
            INSERT INTO thread_events (
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_run_id, caused_by_thread_id, conversation_id, retry_count, max_retries, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8, $9, NULL, NULL, NULL,
                $10, $11, $12, 0, $13, $14, $14
            )
            ON CONFLICT (thread_id, event_type, board_item_id)
            WHERE board_item_id IS NOT NULL
              AND status = 'pending'
              AND event_type IN ('task_routing', 'assignment_execution')
            DO UPDATE SET
                priority = LEAST(thread_events.priority, EXCLUDED.priority),
                payload = EXCLUDED.payload,
                available_at = LEAST(thread_events.available_at, EXCLUDED.available_at),
                conversation_id = COALESCE(thread_events.conversation_id, EXCLUDED.conversation_id),
                max_retries = GREATEST(thread_events.max_retries, EXCLUDED.max_retries),
                updated_at = EXCLUDED.updated_at
            RETURNING
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_run_id, caused_by_thread_id, conversation_id, retry_count, max_retries, created_at, updated_at
            "#,
            self.id,
            self.deployment_id,
            self.thread_id,
            self.board_item_id,
            self.event_type,
            self.status,
            self.priority,
            self.payload,
            available_at,
            self.caused_by_run_id,
            self.caused_by_thread_id,
            self.conversation_id,
            self.max_retries,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(event)
    }

    fn validate(&self) -> Result<(), AppError> {
        match self.event_type.as_str() {
            models::thread_event::event_type::TASK_ROUTING => {
                let board_item_id = self.board_item_id.ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "{} event requires board_item_id",
                        self.event_type
                    ))
                })?;
                let payload: models::thread_event::TaskRoutingEventPayload =
                    serde_json::from_value(self.payload.clone()).map_err(|e| {
                        AppError::BadRequest(format!(
                            "{} event payload is invalid: {}",
                            self.event_type, e
                        ))
                    })?;
                if payload.board_item_id != board_item_id {
                    return Err(AppError::BadRequest(format!(
                        "{} event board_item_id mismatch: envelope={} payload={}",
                        self.event_type, board_item_id, payload.board_item_id
                    )));
                }
            }
            models::thread_event::event_type::ASSIGNMENT_EXECUTION => {
                let board_item_id = self.board_item_id.ok_or_else(|| {
                    AppError::BadRequest(format!(
                        "{} event requires board_item_id",
                        self.event_type
                    ))
                })?;
                let payload: models::thread_event::ThreadAssignmentEventPayload =
                    serde_json::from_value(self.payload.clone()).map_err(|e| {
                        AppError::BadRequest(format!(
                            "{} event payload is invalid: {}",
                            self.event_type, e
                        ))
                    })?;
                if payload.assignment_id <= 0 {
                    return Err(AppError::BadRequest(format!(
                        "{} event requires valid assignment_id",
                        self.event_type
                    )));
                }
                let _ = board_item_id;
            }
            _ => {}
        }

        Ok(())
    }
}

impl DispatchThreadEventCommand {
    pub fn new(enqueue: EnqueueThreadEventCommand) -> Self {
        Self {
            enqueue,
            preferred_agent_id: None,
        }
    }

    pub fn with_agent_id(mut self, agent_id: i64) -> Self {
        self.preferred_agent_id = Some(agent_id);
        self
    }

    pub async fn execute_with_deps<D>(self, deps: &D) -> Result<DispatchThreadEventResult, AppError>
    where
        D: HasDbRouter + HasNatsJetStreamProvider + HasIdProvider + ?Sized,
    {
        let event = self.enqueue.execute_with_db(deps.writer_pool()).await?;
        let thread_state = GetAgentThreadStateQuery::new(event.thread_id, event.deployment_id)
            .execute_with_db(deps.reader_pool(ReadConsistency::Strong))
            .await?;

        let wake_disposition = wake_disposition_for_thread_event(
            &thread_state.status,
            &event.event_type,
            thread_state.reusable,
            thread_state.accepts_assignments,
        );
        if wake_disposition != ThreadEventWakeDisposition::Published {
            return Ok(DispatchThreadEventResult {
                event,
                wake_disposition,
            });
        }

        if let Some(agent_id) = self.preferred_agent_id {
            UpsertThreadAgentAssignmentCommand::new(event.thread_id, agent_id)
                .execute_with_db(deps.writer_pool())
                .await?;
        }

        PublishThreadScheduleCommand::new(event.deployment_id, event.thread_id)
            .execute_with_deps(deps)
            .await?;

        Ok(DispatchThreadEventResult {
            event,
            wake_disposition: ThreadEventWakeDisposition::Published,
        })
    }
}

impl UpdateThreadEventStateCommand {
    pub fn new(event_id: i64, status: String) -> Self {
        Self {
            event_id,
            status,
            caused_by_run_id: None,
            claimed_at: None,
            completed_at: None,
            failed_at: None,
        }
    }

    pub fn with_caused_by_run_id(mut self, run_id: i64) -> Self {
        self.caused_by_run_id = Some(run_id);
        self
    }

    pub fn mark_claimed(mut self) -> Self {
        self.claimed_at = Some(Utc::now());
        self
    }

    pub fn mark_completed(mut self) -> Self {
        self.completed_at = Some(Utc::now());
        self
    }

    pub fn mark_failed(mut self) -> Self {
        self.failed_at = Some(Utc::now());
        self
    }

    pub async fn execute_with_db<'e, E>(self, executor: E) -> Result<ThreadEvent, AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres>,
    {
        let now = Utc::now();
        let event = sqlx::query_as!(
            ThreadEvent,
            r#"
            UPDATE thread_events
            SET
                status = $2,
                caused_by_run_id = COALESCE($3, caused_by_run_id),
                claimed_at = COALESCE($4, claimed_at),
                completed_at = COALESCE($5, completed_at),
                failed_at = COALESCE($6, failed_at),
                updated_at = $7
            WHERE id = $1
            RETURNING
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_run_id, caused_by_thread_id, conversation_id, retry_count, max_retries, created_at, updated_at
            "#,
            self.event_id,
            self.status,
            self.caused_by_run_id,
            self.claimed_at,
            self.completed_at,
            self.failed_at,
            now,
        )
        .fetch_one(executor)
        .await?;

        Ok(event)
    }
}

pub struct RecoveredStaleEvent {
    pub id: i64,
    pub deployment_id: i64,
    pub thread_id: i64,
}

pub struct RecoverStaleClaimedThreadEventsCommand {
    pub stale_before: chrono::DateTime<Utc>,
}

impl RecoverStaleClaimedThreadEventsCommand {
    pub fn new(stale_before: chrono::DateTime<Utc>) -> Self {
        Self { stale_before }
    }

    pub async fn execute_with_db<'e, E>(
        self,
        executor: E,
    ) -> Result<(Vec<RecoveredStaleEvent>, u64), AppError>
    where
        E: sqlx::Executor<'e, Database = sqlx::Postgres> + Copy,
    {
        let retriable_rows = sqlx::query!(
            r#"
            UPDATE thread_events
            SET status = 'pending',
                retry_count = retry_count + 1,
                claimed_at = NULL,
                updated_at = NOW()
            WHERE status = 'claimed'
              AND claimed_at < $1
              AND retry_count < max_retries
              AND event_type IN ('user_message_received', 'user_input_received', 'approval_response_received')
            RETURNING id, deployment_id, thread_id
            "#,
            self.stale_before,
        )
        .fetch_all(executor)
        .await?;

        let exhausted_rows = sqlx::query!(
            r#"
            UPDATE thread_events
            SET status = 'failed',
                failed_at = NOW(),
                updated_at = NOW()
            WHERE status = 'claimed'
              AND claimed_at < $1
              AND retry_count >= max_retries
              AND event_type IN ('user_message_received', 'user_input_received', 'approval_response_received')
            RETURNING id
            "#,
            self.stale_before,
        )
        .fetch_all(executor)
        .await?;

        let retriable = retriable_rows
            .into_iter()
            .map(|row| RecoveredStaleEvent {
                id: row.id,
                deployment_id: row.deployment_id,
                thread_id: row.thread_id,
            })
            .collect();

        Ok((retriable, exhausted_rows.len() as u64))
    }
}
