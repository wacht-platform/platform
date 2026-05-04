use anyhow::Result;
use commands::MarkStorageAsCleanCommand;
use common::{DodoClient, state::AppState};
use queries::{GetDeploymentProviderSubscriptionQuery, GetDirtyStorageDeploymentsQuery};
use tracing::error;

pub async fn sync_storage_to_dodo(app_state: &AppState) -> Result<String> {
    let dirty_deployments = GetDirtyStorageDeploymentsQuery
        .execute_with_db(app_state.db_router.writer())
        .await?;

    if dirty_deployments.is_empty() {
        return Ok("No dirty deployments".to_string());
    }

    let dodo_client = match DodoClient::new() {
        Ok(client) => client,
        Err(_) => {
            return Ok("Dodo not configured".to_string());
        }
    };

    let mut synced_count = 0;

    for (deployment_id, total_bytes) in dirty_deployments {
        let subscription_info = match GetDeploymentProviderSubscriptionQuery::new(deployment_id)
            .execute_with_db(
                app_state
                    .db_router
                    .reader(common::db_router::ReadConsistency::Strong),
            )
            .await
        {
            Ok(Some(info)) => info,
            Ok(None) => continue,
            Err(e) => {
                error!(
                    "[STORAGE SYNC] failed to get subscription for deployment {}: {}",
                    deployment_id, e
                );
                continue;
            }
        };

        if subscription_info.plan_name == "starter" {
            continue;
        }

        let customer_id = subscription_info.provider_customer_id;

        let storage_kb = (total_bytes as f64 / 1000.0).ceil() as i64;

        let event_id = format!(
            "storage_{}_{}_{}",
            deployment_id,
            chrono::Utc::now().timestamp(),
            app_state.sf.next_id().unwrap_or(0)
        );

        match dodo_client
            .ingest_usage_events(&customer_id, "storage.used", storage_kb, &event_id, true)
            .await
        {
            Ok(_) => {
                MarkStorageAsCleanCommand { deployment_id }
                    .execute_with_db(app_state.db_router.writer())
                    .await?;

                synced_count += 1;
            }
            Err(e) => {
                error!(
                    "[STORAGE SYNC] failed to sync storage for deployment {}: {}",
                    deployment_id, e
                );
            }
        }
    }

    Ok(format!("Synced {} deployments", synced_count))
}
