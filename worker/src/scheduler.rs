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
            interval.tick().await;
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

        let storage_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(3600));
            interval.tick().await;
            loop {
                interval.tick().await;
                info!("Running storage sync job...");

                match jobs::storage_sync::sync_storage_to_dodo(&storage_state).await {
                    Ok(result) => {
                        info!("Storage sync completed: {}", result);
                    }
                    Err(e) => {
                        error!("Storage sync failed: {}", e);
                    }
                }
            }
        });

        let oauth_grant_last_used_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300));
            interval.tick().await;
            loop {
                interval.tick().await;
                match jobs::oauth_grant_last_used_sync::sync_oauth_grant_last_used(
                    &oauth_grant_last_used_state,
                )
                .await
                {
                    Ok(result) => {
                        info!("OAuth grant last_used sync completed: {}", result);
                    }
                    Err(e) => {
                        error!("OAuth grant last_used sync failed: {}", e);
                    }
                }
            }
        });

        let agent_execution_recovery_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1800));
            interval.tick().await;
            loop {
                interval.tick().await;
                info!("Running agent execution recovery job...");

                match jobs::agent_execution_recovery::recover_zombie_agent_executions(
                    &agent_execution_recovery_state,
                )
                .await
                {
                    Ok(result) => {
                        info!("Agent execution recovery completed: {}", result);
                    }
                    Err(e) => {
                        error!("Agent execution recovery failed: {}", e);
                    }
                }
            }
        });

        let project_task_schedule_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1800));
            interval.tick().await;
            loop {
                interval.tick().await;
                info!("Running project task schedule dispatch job...");

                match jobs::project_task_schedule_dispatch::dispatch_due_project_task_schedules(
                    &project_task_schedule_state,
                )
                .await
                {
                    Ok(result) => {
                        info!("Project task schedule dispatch completed: {}", result);
                    }
                    Err(e) => {
                        error!("Project task schedule dispatch failed: {}", e);
                    }
                }
            }
        });

        let due_thread_event_wake_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1800));
            interval.tick().await;
            loop {
                interval.tick().await;
                info!("Running due thread event wake job...");

                match jobs::due_thread_event_wake::wake_due_thread_events(
                    &due_thread_event_wake_state,
                )
                .await
                {
                    Ok(result) => {
                        info!("Due thread event wake completed: {}", result);
                    }
                    Err(e) => {
                        error!("Due thread event wake failed: {}", e);
                    }
                }
            }
        });

        info!("Job scheduler started successfully");
        Ok(())
    }
}
