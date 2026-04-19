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
    let recoverable_failed_for_work = matches!(
        event_type,
        models::thread_event::event_type::TASK_ROUTING
            | models::thread_event::event_type::ASSIGNMENT_EXECUTION
            | models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW
    ) && (reusable || accepts_assignments);

    match thread_status {
        AgentThreadStatus::Running => ThreadEventWakeDisposition::NoopThreadActive,
        AgentThreadStatus::WaitingForInput => match event_type {
            models::thread_event::event_type::USER_MESSAGE_RECEIVED
            | models::thread_event::event_type::USER_INPUT_RECEIVED
            | models::thread_event::event_type::APPROVAL_RESPONSE_RECEIVED
            | models::thread_event::event_type::TASK_ROUTING
            | models::thread_event::event_type::ASSIGNMENT_EXECUTION
            | models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW
            | models::thread_event::event_type::CONTROL_STOP
            | models::thread_event::event_type::CONTROL_INTERRUPT => {
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

impl EnqueueThreadEventCommand {
    pub fn new(id: i64, deployment_id: i64, thread_id: i64, event_type: String) -> Self {
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
        }
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
                caused_by_run_id, caused_by_thread_id, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8, $9, NULL, NULL, NULL,
                $10, $11, $12, $12
            )
            ON CONFLICT (thread_id, event_type, board_item_id)
            WHERE board_item_id IS NOT NULL
              AND status = 'pending'
              AND event_type IN ('task_routing', 'assignment_execution', 'assignment_outcome_review')
            DO UPDATE SET
                priority = LEAST(thread_events.priority, EXCLUDED.priority),
                payload = EXCLUDED.payload,
                available_at = LEAST(thread_events.available_at, EXCLUDED.available_at),
                updated_at = EXCLUDED.updated_at
            RETURNING
                id, deployment_id, thread_id, board_item_id, event_type, status,
                priority, payload, available_at, claimed_at, completed_at, failed_at,
                caused_by_run_id, caused_by_thread_id, created_at, updated_at
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
            models::thread_event::event_type::ASSIGNMENT_EXECUTION
            | models::thread_event::event_type::ASSIGNMENT_OUTCOME_REVIEW => {
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
                caused_by_run_id, caused_by_thread_id, created_at, updated_at
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
