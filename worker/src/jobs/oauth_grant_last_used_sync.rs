use anyhow::Result;
use commands::{Command, SyncOAuthGrantLastUsedBatch};
use common::state::AppState;
use tracing::info;

pub async fn sync_oauth_grant_last_used(app_state: &AppState) -> Result<String> {
    let synced = SyncOAuthGrantLastUsedBatch { batch_size: 100 }
        .execute(app_state)
        .await?;
    if synced == 0 {
        return Ok("No dirty oauth grant last_used entries".to_string());
    }
    info!(
        "[OAUTH GRANT LAST_USED SYNC] synced {} grant usage entries",
        synced
    );
    Ok(format!("Synced {} oauth grant usage entries", synced))
}
