use anyhow::Result;
use commands::PublishThreadScheduleCommand;
use common::{ReadConsistency, state::AppState};
use queries::ListThreadsWithDuePendingThreadEventsQuery;
use tracing::{info, warn};

const DUE_THREAD_EVENT_WAKE_LIMIT: i64 = 200;

pub async fn wake_due_thread_events(app_state: &AppState) -> Result<String> {
    let threads = ListThreadsWithDuePendingThreadEventsQuery::new(DUE_THREAD_EVENT_WAKE_LIMIT)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    if threads.is_empty() {
        return Ok("No due pending thread events".to_string());
    }

    let deps = common::deps::from_app(app_state).nats().id();
    let mut published = 0usize;

    for (deployment_id, thread_id) in threads {
        match PublishThreadScheduleCommand::new(deployment_id, thread_id)
            .execute_with_deps(&deps)
            .await
        {
            Ok(()) => {
                published += 1;
            }
            Err(error) => {
                warn!(
                    deployment_id,
                    thread_id,
                    error = %error,
                    "Failed to publish thread schedule for due pending thread event"
                );
            }
        }
    }

    info!(published, "Due thread event wake scan completed");
    Ok(format!(
        "Published thread schedule for {} threads with due pending events",
        published
    ))
}
