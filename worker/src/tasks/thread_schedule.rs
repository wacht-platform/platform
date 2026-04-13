use anyhow::Result;
use commands::{
    ClaimNextSchedulableThreadEventCommand, ClaimNextSchedulableThreadEventResult,
    PublishAgentExecutionCommand, ReleaseClaimedThreadEventCommand,
};
use common::{ReadConsistency, state::AppState};
use dto::json::ThreadScheduleRequest;
use queries::ResolveThreadExecutionAgentQuery;
use tracing::{info, warn};

fn parse_string_id(field_name: &str, raw_value: &str) -> Result<i64> {
    raw_value
        .parse::<i64>()
        .map_err(|error| anyhow::anyhow!("Invalid {} '{}': {}", field_name, raw_value, error))
}

pub async fn process_thread_schedule(
    app_state: &AppState,
    request: ThreadScheduleRequest,
) -> Result<String> {
    let deployment_id = parse_string_id("deployment_id", &request.deployment_id)?;
    let thread_id = parse_string_id("thread_id", &request.thread_id)?;

    let db_deps = common::deps::from_app(app_state).db();
    let event = match ClaimNextSchedulableThreadEventCommand::new(deployment_id, thread_id)
        .execute_with_deps(&db_deps)
        .await?
    {
        ClaimNextSchedulableThreadEventResult::Claimed(event) => event,
        ClaimNextSchedulableThreadEventResult::NoThreadAvailable => {
            info!(
                deployment_id,
                thread_id, "Thread scheduler no-op: thread unavailable, locked, or archived"
            );
            return Ok(format!(
                "Thread {} unavailable, locked, or archived",
                thread_id
            ));
        }
        ClaimNextSchedulableThreadEventResult::ExistingClaimNotStale {
            event_id,
            claimed_at,
        } => {
            info!(
                deployment_id,
                thread_id,
                event_id,
                ?claimed_at,
                "Thread scheduler no-op: existing claimed event is still in flight"
            );
            return Ok(format!(
                "Thread {} already has in-flight claimed event {}",
                thread_id, event_id
            ));
        }
        ClaimNextSchedulableThreadEventResult::NoPendingEvent => {
            info!(
                deployment_id,
                thread_id, "Thread scheduler no-op: no pending event available"
            );
            return Ok(format!(
                "No pending event available for thread {}",
                thread_id
            ));
        }
        ClaimNextSchedulableThreadEventResult::WakeNotAllowed {
            event_id,
            event_type,
            thread_status,
        } => {
            info!(
                deployment_id,
                thread_id,
                event_id,
                event_type,
                thread_status,
                "Thread scheduler no-op: wake gate blocked dispatch"
            );
            return Ok(format!(
                "Wake blocked for thread {} event {} ({}) while thread status is {}",
                thread_id, event_id, event_type, thread_status
            ));
        }
    };

    let resolved_agent_id = ResolveThreadExecutionAgentQuery::new(thread_id, deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    let Some(agent_id) = resolved_agent_id else {
        warn!(
            deployment_id,
            thread_id,
            event_id = event.id,
            "Thread scheduler no-op: no resolved execution agent"
        );
        ReleaseClaimedThreadEventCommand::new(event.id)
            .execute_with_deps(&db_deps)
            .await?;
        return Ok(format!(
            "Thread {} has no resolved execution agent for event {}",
            thread_id, event.id
        ));
    };

    let publish_deps = common::deps::from_app(app_state).nats().id();
    if let Err(error) = PublishAgentExecutionCommand::from_thread_event(&event, Some(agent_id))?
        .execute_with_deps(&publish_deps)
        .await
    {
        warn!(
            deployment_id,
            thread_id,
            event_id = event.id,
            agent_id,
            error = %error,
            "Thread scheduler failed to publish execution request"
        );
        ReleaseClaimedThreadEventCommand::new(event.id)
            .execute_with_deps(&db_deps)
            .await?;
        return Err(anyhow::anyhow!(
            "Failed to publish event {} for thread {}: {}",
            event.id,
            thread_id,
            error
        ));
    }

    info!(
        deployment_id,
        thread_id,
        event_id = event.id,
        agent_id,
        "Thread scheduler published execution request"
    );
    Ok(format!(
        "Scheduled thread event {} for thread {}",
        event.id, thread_id
    ))
}
