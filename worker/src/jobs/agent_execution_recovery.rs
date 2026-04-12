use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use commands::{
    CreateProjectTaskBoardItemEventCommand, DispatchThreadEventCommand, EnqueueThreadEventCommand,
    PublishThreadScheduleCommand, RecordAgentExecutionRecoveryCommand,
    UpdateAgentExecutionRecoveryStatusCommand, UpdateAgentThreadStateCommand,
    UpdateExecutionRunStateCommand, UpdateThreadEventStateCommand,
};
use common::{ReadConsistency, state::AppState};
use models::{AgentThreadStatus, agent_execution_recovery};
use queries::{
    GetActorProjectByIdQuery, GetAgentThreadStateQuery, GetProjectTaskBoardByIdQuery,
    GetProjectTaskBoardItemByIdQuery, ListStaleClaimedThreadEventsQuery,
    ListStaleExecutionRunsQuery,
};
use redis::AsyncCommands as _;
use serde_json::json;
use tracing::{info, warn};

const STALE_CLAIMED_EVENT_AFTER_MINUTES: i64 = 20;
const STALE_EXECUTION_RUN_AFTER_MINUTES: i64 = 90;
const SCAN_BATCH_LIMIT: i64 = 100;

async fn return_board_item_to_coordinator(
    app_state: &AppState,
    board_item_id: i64,
    deployment_id: i64,
    note: String,
    caused_by_thread_id: i64,
) -> Result<bool> {
    let Some(board_item) = GetProjectTaskBoardItemByIdQuery::new(board_item_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?
    else {
        return Ok(false);
    };

    if matches!(board_item.status.as_str(), "completed" | "cancelled") {
        return Ok(false);
    }

    let Some(board) = GetProjectTaskBoardByIdQuery::new(board_item.board_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?
    else {
        return Ok(false);
    };

    let Some(project) = GetActorProjectByIdQuery::new(board.project_id, board.deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?
    else {
        return Ok(false);
    };
    let Some(coordinator_thread_id) = project.coordinator_thread_id else {
        return Ok(false);
    };

    let coordinator = GetAgentThreadStateQuery::new(coordinator_thread_id, board.deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    let deps = common::deps::from_app(app_state).db().nats().id();
    if coordinator.status == models::AgentThreadStatus::Failed {
        UpdateAgentThreadStateCommand::new(coordinator_thread_id, coordinator.deployment_id)
            .with_status(models::AgentThreadStatus::Interrupted)
            .execute_with_deps(&deps)
            .await?;
    }

    CreateProjectTaskBoardItemEventCommand {
        id: app_state.sf.next_id()? as i64,
        board_item_id: board_item.id,
        thread_id: Some(coordinator_thread_id),
        execution_run_id: None,
        event_type: "task_returned_to_coordinator".to_string(),
        summary: "Task returned to coordinator for rerouting".to_string(),
        body_markdown: None,
        details: serde_json::json!({
            "board_item_id": board_item.id.to_string(),
            "task_key": board_item.task_key,
            "status": board_item.status,
            "note": note,
        }),
    }
    .execute_with_db(app_state.db_router.writer())
    .await?;

    let payload = models::thread_event::TaskRoutingEventPayload {
        board_item_id: board_item.id,
    };

    DispatchThreadEventCommand::new(
        EnqueueThreadEventCommand::new(
            app_state.sf.next_id()? as i64,
            coordinator.deployment_id,
            coordinator_thread_id,
            models::thread_event::event_type::TASK_ROUTING.to_string(),
        )
        .with_board_item_id(board_item.id)
        .with_priority(15)
        .with_caused_by_thread_id(caused_by_thread_id)
        .with_payload(serde_json::to_value(payload)?),
    )
    .execute_with_deps(&deps)
    .await?;

    Ok(true)
}

pub async fn recover_zombie_agent_executions(app_state: &AppState) -> Result<String> {
    let stale_claimed_before =
        Utc::now() - ChronoDuration::minutes(STALE_CLAIMED_EVENT_AFTER_MINUTES);
    let stale_run_before = Utc::now() - ChronoDuration::minutes(STALE_EXECUTION_RUN_AFTER_MINUTES);

    let claimed_candidates =
        ListStaleClaimedThreadEventsQuery::new(stale_claimed_before, SCAN_BATCH_LIMIT)
            .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
            .await?;
    let run_candidates = ListStaleExecutionRunsQuery::new(stale_run_before, SCAN_BATCH_LIMIT)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    let mut recorded = 0usize;
    let mut requeued = 0usize;
    let mut unresolved_runs = 0usize;

    for candidate in claimed_candidates {
        if thread_lock_exists(app_state, candidate.thread_id).await? {
            continue;
        }

        let detail = json!({
            "kind": "claimed_event_stale",
            "thread_status": candidate.thread_status,
            "thread_updated_at": candidate.thread_updated_at,
            "event_type": candidate.event_type,
            "claimed_at": candidate.claimed_at,
            "execution_run_status": candidate.execution_run_status,
            "execution_run_started_at": candidate.execution_run_started_at,
            "execution_run_updated_at": candidate.execution_run_updated_at,
        });

        let entry = RecordAgentExecutionRecoveryCommand {
            thread_id: candidate.thread_id,
            thread_event_id: Some(candidate.thread_event_id),
            execution_run_id: candidate.execution_run_id,
            reason_code: agent_execution_recovery::reason_code::CLAIMED_EVENT_STALE.to_string(),
            reason_detail: detail,
        }
        .execute_with_deps(&common::deps::from_app(app_state).db().id())
        .await?;
        recorded += 1;

        if let Some(run_id) = candidate.execution_run_id {
            if matches!(candidate.execution_run_status.as_deref(), Some("running")) {
                let _ = UpdateExecutionRunStateCommand::new(run_id, candidate.deployment_id)
                    .with_status("failed".to_string())
                    .mark_failed()
                    .execute_with_db(app_state.db_router.writer())
                    .await;
            }
        }

        if candidate.thread_status == AgentThreadStatus::Running.to_string() {
            let deps = common::deps::from_app(app_state).db().nats().id();
            UpdateAgentThreadStateCommand::new(candidate.thread_id, candidate.deployment_id)
                .with_status(AgentThreadStatus::Interrupted)
                .execute_with_deps(&deps)
                .await?;
        }

        UpdateThreadEventStateCommand::new(
            candidate.thread_event_id,
            models::thread_event::status::FAILED.to_string(),
        )
        .mark_failed()
        .execute_with_db(app_state.db_router.writer())
        .await?;

        let rerouted_to_coordinator = if let Some(board_item_id) = candidate.board_item_id {
            return_board_item_to_coordinator(
                app_state,
                board_item_id,
                candidate.deployment_id,
                "Recovered stale claimed event tied to a dead execution path; returned task to coordinator for rerouting"
                    .to_string(),
                candidate.thread_id,
            )
            .await?
        } else {
            PublishThreadScheduleCommand::new(candidate.deployment_id, candidate.thread_id)
                .execute_with_deps(&common::deps::from_app(app_state).nats().id())
                .await?;
            false
        };

        UpdateAgentExecutionRecoveryStatusCommand::new(entry.id)
            .with_status(agent_execution_recovery::recovery_status::REQUEUED.to_string())
            .with_reason_detail(json!({
                "outcome": if rerouted_to_coordinator {
                    "failed_stale_event_and_returned_board_item_to_coordinator"
                } else {
                    "failed_stale_event_and_rescheduled_thread"
                },
                "thread_event_id": candidate.thread_event_id,
                "execution_run_id": candidate.execution_run_id,
                "board_item_id": candidate.board_item_id,
            }))
            .increment_retry_count()
            .mark_attempted_now()
            .mark_resolved_now()
            .execute_with_deps(&common::deps::from_app(app_state).db())
            .await?;
        requeued += 1;
    }

    for candidate in run_candidates {
        if thread_lock_exists(app_state, candidate.thread_id).await? {
            continue;
        }

        let detail = json!({
            "kind": "execution_run_stuck",
            "thread_status": candidate.thread_status,
            "thread_updated_at": candidate.thread_updated_at,
            "execution_run_started_at": candidate.execution_run_started_at,
            "execution_run_updated_at": candidate.execution_run_updated_at,
            "note": "No claimed thread event still references this running execution run",
        });

        let entry = RecordAgentExecutionRecoveryCommand {
            thread_id: candidate.thread_id,
            thread_event_id: None,
            execution_run_id: Some(candidate.execution_run_id),
            reason_code: agent_execution_recovery::reason_code::EXECUTION_RUN_STUCK.to_string(),
            reason_detail: detail,
        }
        .execute_with_deps(&common::deps::from_app(app_state).db().id())
        .await?;
        recorded += 1;

        warn!(
            deployment_id = candidate.deployment_id,
            thread_id = candidate.thread_id,
            execution_run_id = candidate.execution_run_id,
            recovery_entry_id = entry.id,
            "Detected stale execution run without a claimed thread event; recorded DLQ entry for manual follow-up"
        );
        unresolved_runs += 1;
    }

    info!(
        recorded,
        requeued, unresolved_runs, "Agent execution recovery scan completed"
    );

    Ok(format!(
        "Recorded {} zombie candidates, requeued {}, left {} unresolved run cases in the recovery queue",
        recorded, requeued, unresolved_runs
    ))
}

async fn thread_lock_exists(app_state: &AppState, thread_id: i64) -> Result<bool> {
    let key = format!("agent:thread_execution_lock:{}", thread_id);
    let mut conn = app_state
        .redis_client
        .get_multiplexed_async_connection()
        .await?;
    let current: Option<String> = conn.get(key).await?;
    Ok(current.is_some())
}
