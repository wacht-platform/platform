mod rclone;
mod state;

use self::rclone::*;
use self::state::*;
pub(crate) use self::state::detect_local_execution_base_path;

use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use commands::{ResolveDeploymentStorageCommand, ResolvedDeploymentStorage};
use common::deps;
use common::error::AppError;
use common::state::AppState;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::Mutex;

static DEPLOYMENT_MOUNT_MANAGER: OnceLock<Arc<DeploymentMountManager>> = OnceLock::new();
static WORKER_MOUNT_HOLDER_ID: OnceLock<String> = OnceLock::new();

const DEPLOYMENT_MOUNT_IDLE_TIMEOUT_SECS: u64 = 1800;
const MOUNT_STATE_LOCK_WAIT_SECS: u64 = 30;
const MOUNT_STATE_LOCK_RETRY_MS: u64 = 100;
const RCLONE_MOUNT_READY_INITIAL_WAIT_MS: u64 = 800;
const RCLONE_MOUNT_READY_MAX_ATTEMPTS: usize = 6;

pub async fn acquire_deployment_root(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<DeploymentMountLease, AppError> {
    deployment_mount_manager()
        .acquire(app_state, deployment_id)
        .await
}

pub async fn cleanup_startup_mounts() -> Result<(), AppError> {
    let _ = deployment_mount_base_path();
    let _ = mount_state_base_path();
    let _ = rclone_cache_base_path();

    cleanup_stale_mount_lock_files().await?;
    let retained_deployments = cleanup_startup_mount_states().await?;
    cleanup_orphan_mount_directories(&retained_deployments).await?;

    Ok(())
}

pub async fn heartbeat_deployment_root(deployment_id: i64) -> Result<(), AppError> {
    let _state_lock = MountStateLock::acquire(deployment_id).await?;
    let Some(mut mount_state) = load_mount_state(deployment_id).await? else {
        return Ok(());
    };

    prune_stale_holders(&mut mount_state).await;
    let now = unix_time_ms();
    mount_state.last_activity_unix_ms = now;

    if let Some(holder) = mount_state.holders.get_mut(&current_mount_holder_id()) {
        holder.pid = std::process::id();
        holder.last_used_unix_ms = now;
    }

    save_mount_state(deployment_id, &mount_state).await
}

fn deployment_mount_manager() -> Arc<DeploymentMountManager> {
    DEPLOYMENT_MOUNT_MANAGER
        .get_or_init(|| Arc::new(DeploymentMountManager::new()))
        .clone()
}

struct DeploymentMountManager {
    mounts: Mutex<HashMap<i64, ManagedMount>>,
}

struct DeploymentRemoteMountInfo {
    command_arg: String,
    redacted_display: String,
    identity_fingerprint: String,
    sensitive_values: Vec<String>,
}

impl DeploymentMountManager {
    fn new() -> Self {
        Self {
            mounts: Mutex::new(HashMap::new()),
        }
    }

    async fn acquire(
        self: &Arc<Self>,
        app_state: &AppState,
        deployment_id: i64,
    ) -> Result<DeploymentMountLease, AppError> {
        let deps = deps::from_app(app_state).db().enc();
        let storage = ResolveDeploymentStorageCommand::new(deployment_id)
            .execute_with_deps(&deps)
            .await?;
        let mount_root = deployment_mount_path(deployment_id);
        let remote = deployment_remote_mount_info(&storage, deployment_id)?;
        let holder_id = current_mount_holder_id();
        let now = unix_time_ms();

        let fast_path = {
            let mut mounts = self.mounts.lock().await;
            match mounts.get_mut(&deployment_id) {
                Some(existing) if existing.ref_count > 0 => {
                    existing.ref_count += 1;
                    Some(existing.root_path.clone())
                }
                Some(_) => {
                    mounts.remove(&deployment_id);
                    None
                }
                None => None,
            }
        };

        if let Some(root_path) = fast_path {
            let _state_lock = MountStateLock::acquire(deployment_id).await?;
            let mut mount_state = load_mount_state(deployment_id)
                .await?
                .unwrap_or_else(|| MountState::new(deployment_id, &root_path, &remote));
            prune_stale_holders(&mut mount_state).await;
            mount_state.ensure_identity(deployment_id, &root_path, &remote);
            touch_holder(&mut mount_state, &holder_id, now);
            save_mount_state(deployment_id, &mount_state).await?;

            return Ok(DeploymentMountLease::managed(
                self.clone(),
                deployment_id,
                root_path,
            ));
        }

        let _state_lock = MountStateLock::acquire(deployment_id).await?;
        let mut mount_state = load_mount_state(deployment_id)
            .await?
            .unwrap_or_else(|| MountState::new(deployment_id, &mount_root, &remote));
        prune_stale_holders(&mut mount_state).await;

        let active_leases = mount_state.total_active_leases();
        let mount_is_live = is_mount_live(&mount_root).await?;
        let identity_matches = mount_state.mount_root == mount_root.to_string_lossy()
            && mount_state.remote_identity_fingerprint == remote.identity_fingerprint;

        if mount_is_live && identity_matches {
            mount_state.ensure_identity(deployment_id, &mount_root, &remote);
        } else {
            if active_leases > 0 {
                return Err(AppError::Internal(format!(
                    "Deployment mount '{}' is already in use and cannot be remounted safely",
                    mount_root.display()
                )));
            }

            cleanup_mount_artifacts(deployment_id, &mount_root).await?;
            prepare_mount_path(&mount_root).await?;
            mount_with_rclone(&mount_root, deployment_id, &storage).await?;
            mount_state = MountState::new(deployment_id, &mount_root, &remote);
        }

        touch_holder(&mut mount_state, &holder_id, now);
        save_mount_state(deployment_id, &mount_state).await?;

        let mut mounts = self.mounts.lock().await;
        mounts.insert(
            deployment_id,
            ManagedMount {
                root_path: mount_root.clone(),
                ref_count: 1,
                holder_id,
            },
        );

        Ok(DeploymentMountLease::managed(
            self.clone(),
            deployment_id,
            mount_root,
        ))
    }

    async fn release(self: &Arc<Self>, deployment_id: i64) -> Result<(), AppError> {
        let removal = {
            let mut mounts = self.mounts.lock().await;
            match mounts.get_mut(&deployment_id) {
                Some(mount) if mount.ref_count > 1 => {
                    mount.ref_count -= 1;
                    Some((mount.root_path.clone(), mount.holder_id.clone(), true))
                }
                Some(_) => mounts
                    .remove(&deployment_id)
                    .map(|mount| (mount.root_path, mount.holder_id, false)),
                None => return Ok(()),
            }
        };

        let Some((root_path, holder_id, still_active_in_process)) = removal else {
            return Ok(());
        };

        let _state_lock = MountStateLock::acquire(deployment_id).await?;
        let now = unix_time_ms();
        let mut total_active_leases = 0usize;

        if let Some(mut mount_state) = load_mount_state(deployment_id).await? {
            prune_stale_holders(&mut mount_state).await;
            mount_state.ensure_mount_root(deployment_id, &root_path);

            let remove_holder = if let Some(holder) = mount_state.holders.get_mut(&holder_id) {
                holder.pid = std::process::id();
                holder.active_leases = holder.active_leases.saturating_sub(1);
                holder.last_used_unix_ms = now;
                holder.active_leases == 0
            } else {
                false
            };

            if remove_holder {
                mount_state.holders.remove(&holder_id);
            }

            mount_state.last_activity_unix_ms = now;
            total_active_leases = mount_state.total_active_leases();
            save_mount_state(deployment_id, &mount_state).await?;
        }

        if still_active_in_process {
            return Ok(());
        }

        if total_active_leases == 0 {
            self.schedule_idle_cleanup(deployment_id, now);
        }

        Ok(())
    }

    fn schedule_idle_cleanup(self: &Arc<Self>, deployment_id: i64, idle_since_unix_ms: u64) {
        let manager = self.clone();
        tokio::spawn(async move {
            let sleep_for = remaining_idle_duration(idle_since_unix_ms);
            if !sleep_for.is_zero() {
                tokio::time::sleep(sleep_for).await;
            }

            if let Err(error) = manager
                .cleanup_if_idle(deployment_id, idle_since_unix_ms)
                .await
            {
                let _ = error;
            }
        });
    }

    async fn cleanup_if_idle(
        self: &Arc<Self>,
        deployment_id: i64,
        expected_last_activity_unix_ms: u64,
    ) -> Result<(), AppError> {
        if self.has_in_process_mount(deployment_id).await {
            return Ok(());
        }

        let _state_lock = MountStateLock::acquire(deployment_id).await?;
        let Some(mut mount_state) = load_mount_state(deployment_id).await? else {
            cleanup_orphan_mount_directory(&deployment_mount_path(deployment_id)).await?;
            return Ok(());
        };

        prune_stale_holders(&mut mount_state).await;
        let total_active_leases = mount_state.total_active_leases();

        if total_active_leases > 0 {
            save_mount_state(deployment_id, &mount_state).await?;
            return Ok(());
        }

        if mount_state.last_activity_unix_ms != expected_last_activity_unix_ms {
            save_mount_state(deployment_id, &mount_state).await?;
            return Ok(());
        }

        if !idle_timeout_elapsed(mount_state.last_activity_unix_ms) {
            save_mount_state(deployment_id, &mount_state).await?;
            return Ok(());
        }

        let mount_root = PathBuf::from(&mount_state.mount_root);
        cleanup_mount_artifacts(deployment_id, &mount_root).await?;
        Ok(())
    }

    async fn has_in_process_mount(&self, deployment_id: i64) -> bool {
        self.mounts
            .lock()
            .await
            .get(&deployment_id)
            .map(|mount| mount.ref_count > 0)
            .unwrap_or(false)
    }
}

struct ManagedMount {
    root_path: PathBuf,
    ref_count: usize,
    holder_id: String,
}

#[derive(Clone)]
pub struct DeploymentMountLease {
    root_path: PathBuf,
    manager: Arc<DeploymentMountManager>,
    deployment_id: i64,
    released: Arc<AtomicBool>,
}

impl DeploymentMountLease {
    fn managed(
        manager: Arc<DeploymentMountManager>,
        deployment_id: i64,
        root_path: PathBuf,
    ) -> Self {
        Self {
            root_path,
            manager,
            deployment_id,
            released: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub async fn release(&self) -> Result<(), AppError> {
        if self.released.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        self.manager.release(self.deployment_id).await
    }
}

impl Drop for DeploymentMountLease {
    fn drop(&mut self) {
        if self.released.swap(true, Ordering::SeqCst) {
            return;
        }

        let manager = self.manager.clone();
        let deployment_id = self.deployment_id;

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                if let Err(error) = manager.release(deployment_id).await {
                    let _ = error;
                }
            });
        } else {
        }
    }
}

