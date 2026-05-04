use anyhow::Result;
use common::state::AppState;
use std::time::Duration;
use tokio::time;
use tracing::error;

use crate::jobs;

pub struct JobScheduler {
    app_state: AppState,
}

impl JobScheduler {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn start(&self) -> Result<()> {
        // Spawn billing sync job (runs every 10 minutes)
        let billing_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(600)); // 10 minutes
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(e) =
                    jobs::billing_sync::sync_redis_to_postgres_and_dodo(&billing_state).await
                {
                    error!("Billing sync failed: {}", e);
                }
            }
        });

        let storage_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(3600));
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(e) = jobs::storage_sync::sync_storage_to_dodo(&storage_state).await {
                    error!("Storage sync failed: {}", e);
                }
            }
        });

        let oauth_grant_last_used_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300));
            interval.tick().await;
            loop {
                interval.tick().await;
                if let Err(e) = jobs::oauth_grant_last_used_sync::sync_oauth_grant_last_used(
                    &oauth_grant_last_used_state,
                )
                .await
                {
                    error!("OAuth grant last_used sync failed: {}", e);
                }
            }
        });

        let project_task_schedule_state = self.app_state.clone();
        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300));
            loop {
                interval.tick().await;
                if let Err(e) =
                    jobs::project_task_schedule_dispatch::dispatch_due_project_task_schedules(
                        &project_task_schedule_state,
                    )
                    .await
                {
                    error!("Project task schedule dispatch failed: {}", e);
                }
            }
        });

        let board_reconciler_state = self.app_state.clone();
        tokio::spawn(async move {
            let owner = board_reconciler_state
                .sf
                .next_id()
                .map(|id| id.to_string())
                .unwrap_or_else(|_| std::process::id().to_string());
            loop {
                match jobs::board_reconciler::acquire_lease(&board_reconciler_state, &owner).await {
                    Ok(true) => {
                        if let Err(e) = jobs::board_reconciler::reconcile_stale_board_items(
                            &board_reconciler_state,
                        )
                        .await
                        {
                            error!("Board reconciler tick failed: {}", e);
                        }
                        if let Err(e) =
                            jobs::board_reconciler::release_lease(&board_reconciler_state, &owner)
                                .await
                        {
                            error!("Board reconciler failed to release lease: {}", e);
                        }
                    }
                    Ok(false) => {
                        // Another worker owns the lease this tick.
                    }
                    Err(e) => {
                        error!("Board reconciler failed to acquire lease: {}", e);
                    }
                }
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
        });

        // Event-log dispatcher: publishes pending event_log rows to NATS.
        // Wakes via NATS subject `agent.outbox.wake.>` + 30s safety poll.
        let dispatcher_state = self.app_state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = jobs::event_dispatcher::run(dispatcher_state.clone()).await {
                    error!(error = %e, "event dispatcher exited; restarting in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        });

        // Work-lease recovery cron: 60s sweep that releases expired leases
        // and dead-letters events that exhausted their retry budget.
        let lease_recovery_state = self.app_state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = jobs::work_lease_recovery::run(lease_recovery_state.clone()).await
                {
                    error!(error = %e, "work-lease recovery exited; restarting in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        });

        let stuck_state = self.app_state.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = jobs::stuck_assignment_sweeper::run(stuck_state.clone()).await {
                    error!(error = %e, "stuck-assignment sweeper exited; restarting in 5s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        });

        Ok(())
    }
}
