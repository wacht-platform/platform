use anyhow::Result;
use chrono::Utc;
use commands::MaterializeProjectTaskScheduleCommand;
use common::{ReadConsistency, state::AppState};
use queries::ListDueProjectTaskScheduleIdsQuery;
use tracing::{info, warn};

const DUE_SCHEDULE_SCAN_LIMIT: i64 = 100;

pub async fn dispatch_due_project_task_schedules(app_state: &AppState) -> Result<String> {
    let now = Utc::now();
    let due_schedule_ids = ListDueProjectTaskScheduleIdsQuery::new(now, DUE_SCHEDULE_SCAN_LIMIT)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await?;

    println!(
        "[schedule_debug] dispatch scan at {} found {} due schedules: {:?}",
        now,
        due_schedule_ids.len(),
        due_schedule_ids
    );

    if due_schedule_ids.is_empty() {
        return Ok("No due project task schedules".to_string());
    }

    let mut materialized = 0usize;
    let deps = common::deps::from_app(app_state).db().nats().id();

    for schedule_id in due_schedule_ids {
        match MaterializeProjectTaskScheduleCommand::new(schedule_id)
            .execute_with_deps(&deps)
            .await
        {
            Ok(Some(_)) => {
                materialized += 1;
            }
            Ok(None) => {}
            Err(error) => {
                println!(
                    "[schedule_debug] schedule_id={} ERROR during materialize: {}",
                    schedule_id, error
                );
                warn!(schedule_id, error = %error, "Failed to materialize due project task schedule");
            }
        }
    }

    info!(
        materialized,
        "Project task schedule dispatch scan completed"
    );
    Ok(format!("Queued {materialized} project task schedules"))
}
