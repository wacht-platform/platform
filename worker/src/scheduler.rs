use anyhow::Result;
use common::state::AppState;
use std::time::Duration;
use tokio::time;
use tracing::{error, info};

use crate::jobs;

pub struct JobScheduler {
    app_state: AppState,
}

impl JobScheduler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting job scheduler...");

        // Spawn billing sync job (runs every 10 minutes)
        let billing_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(600)); // 10 minutes
            loop {
                interval.tick().await;
                info!("Running billing sync job...");

                match jobs::billing_sync::sync_redis_to_postgres_and_dodo(&billing_state).await {
                    Ok(result) => {
                        info!("Billing sync completed: {}", result);
                    }
                    Err(e) => {
                        error!("Billing sync failed: {}", e);
                    }
                }
            }
        });

        info!("Job scheduler started successfully");
        Ok(())
    }
}
