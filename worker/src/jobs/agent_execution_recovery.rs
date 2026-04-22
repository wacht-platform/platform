use anyhow::Result;
use chrono::{Duration as ChronoDuration, Utc};
use commands::{PublishThreadScheduleCommand, RecoverStaleClaimedThreadEventsCommand};
use common::state::AppState;
use tracing::info;

const STALE_CLAIMED_EVENT_AFTER_MINUTES: i64 = 10;

pub async fn recover_zombie_agent_executions(app_state: &AppState) -> Result<String> {
    let stale_before = Utc::now() - ChronoDuration::minutes(STALE_CLAIMED_EVENT_AFTER_MINUTES);

    let (retriable, exhausted_count) = RecoverStaleClaimedThreadEventsCommand::new(stale_before)
        .execute_with_db(app_state.db_router.writer())
        .await?;

    let nats_deps = common::deps::from_app(app_state).nats().id();
    for row in &retriable {
        if let Err(error) = PublishThreadScheduleCommand::new(row.deployment_id, row.thread_id)
            .execute_with_deps(&nats_deps)
            .await
        {
            tracing::warn!(
                deployment_id = row.deployment_id,
                thread_id = row.thread_id,
                event_id = row.id,
                %error,
                "Failed to re-publish thread_schedule for recovered event",
            );
        }
    }

    let retriable_count = retriable.len();
    info!(
        retriable = retriable_count,
        exhausted = exhausted_count,
        "Agent execution recovery scan completed"
    );

    Ok(format!(
        "Recovered {} stale events, marked {} as terminally failed",
        retriable_count, exhausted_count
    ))
}
