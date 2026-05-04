use anyhow::Result;
use commands::SyncOAuthGrantLastUsedBatch;
use common::state::AppState;

pub async fn sync_oauth_grant_last_used(app_state: &AppState) -> Result<String> {
    let sync_command = SyncOAuthGrantLastUsedBatch { batch_size: 100 };
    let deps = common::deps::from_app(app_state).db().redis();
    let synced = sync_command.execute_with_deps(&deps).await?;
    if synced == 0 {
        return Ok("No dirty oauth grant last_used entries".to_string());
    }
    Ok(format!("Synced {} oauth grant usage entries", synced))
}
