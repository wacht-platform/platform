use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct MountState {
    pub(super) deployment_id: i64,
    pub(super) mount_root: String,
    #[serde(default)]
    remote_display: String,
    #[serde(default)]
    pub(super) remote_identity_fingerprint: String,
    pub(super) last_activity_unix_ms: u64,
    pub(super) holders: HashMap<String, MountHolder>,
}

impl MountState {
    pub(super) fn new(
        deployment_id: i64,
        mount_root: &Path,
        remote: &DeploymentRemoteMountInfo,
    ) -> Self {
        Self {
            deployment_id,
            mount_root: mount_root.to_string_lossy().to_string(),
            remote_display: remote.redacted_display.clone(),
            remote_identity_fingerprint: remote.identity_fingerprint.clone(),
            last_activity_unix_ms: unix_time_ms(),
            holders: HashMap::new(),
        }
    }

    pub(super) fn ensure_identity(
        &mut self,
        deployment_id: i64,
        mount_root: &Path,
        remote: &DeploymentRemoteMountInfo,
    ) {
        self.deployment_id = deployment_id;
        self.mount_root = mount_root.to_string_lossy().to_string();
        self.remote_display = remote.redacted_display.clone();
        self.remote_identity_fingerprint = remote.identity_fingerprint.clone();
    }

    pub(super) fn ensure_mount_root(&mut self, deployment_id: i64, mount_root: &Path) {
        self.deployment_id = deployment_id;
        self.mount_root = mount_root.to_string_lossy().to_string();
    }

    pub(super) fn total_active_leases(&self) -> usize {
        self.holders
            .values()
            .map(|holder| holder.active_leases)
            .sum()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct MountHolder {
    pub(super) pid: u32,
    pub(super) active_leases: usize,
    pub(super) last_used_unix_ms: u64,
}

impl MountHolder {
    pub(super) fn current() -> Self {
        Self {
            pid: std::process::id(),
            active_leases: 0,
            last_used_unix_ms: unix_time_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct MountStateLockOwner {
    pub(super) pid: u32,
    acquired_at_unix_ms: u64,
}

pub(super) struct MountStateLock {
    path: PathBuf,
}

impl MountStateLock {
    pub(super) async fn acquire(deployment_id: i64) -> Result<Self, AppError> {
        let path = mount_state_lock_path(deployment_id);
        let parent = path.parent().ok_or_else(|| {
            AppError::Internal("Failed to determine mount state lock directory".to_string())
        })?;
        fs::create_dir_all(parent).await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to create mount state directory '{}': {}",
                parent.display(),
                e
            ))
        })?;

        let deadline =
            tokio::time::Instant::now() + Duration::from_secs(MOUNT_STATE_LOCK_WAIT_SECS);
        let payload = serde_json::to_vec(&MountStateLockOwner {
            pid: std::process::id(),
            acquired_at_unix_ms: unix_time_ms(),
        })
        .map_err(|e| {
            AppError::Internal(format!("Failed to serialize mount state lock owner: {}", e))
        })?;

        loop {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .await
            {
                Ok(mut file) => {
                    file.write_all(&payload).await.map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to write mount state lock '{}': {}",
                            path.display(),
                            e
                        ))
                    })?;
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                    if mount_state_lock_is_stale(&path).await {
                        let _ = fs::remove_file(&path).await;
                        continue;
                    }

                    if tokio::time::Instant::now() >= deadline {
                        return Err(AppError::Internal(format!(
                            "Timed out waiting for deployment mount state lock '{}'",
                            path.display()
                        )));
                    }

                    tokio::time::sleep(Duration::from_millis(MOUNT_STATE_LOCK_RETRY_MS)).await;
                }
                Err(error) => {
                    return Err(AppError::Internal(format!(
                        "Failed to acquire mount state lock '{}': {}",
                        path.display(),
                        error
                    )));
                }
            }
        }
    }
}

impl Drop for MountStateLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

pub(crate) fn detect_local_execution_base_path() -> PathBuf {
    let root = PathBuf::from("/tmp/wacht-agent-executions");
    let _ = std::fs::create_dir_all(&root);
    root
}

pub(super) fn wacht_agent_base_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let root = PathBuf::from(home).join(".wacht-agent");
        let _ = std::fs::create_dir_all(&root);
        return root;
    }

    let fallback = PathBuf::from("/tmp/wacht-agent");
    let _ = std::fs::create_dir_all(&fallback);
    fallback
}

pub(super) fn deployment_mount_base_path() -> PathBuf {
    let root = wacht_agent_base_path().join("mounts");
    let _ = std::fs::create_dir_all(&root);
    root
}

pub(super) fn deployment_mount_path(deployment_id: i64) -> PathBuf {
    deployment_mount_base_path().join(deployment_id.to_string())
}

pub(super) fn mount_state_base_path() -> PathBuf {
    let root = wacht_agent_base_path().join("mount-state");
    let _ = std::fs::create_dir_all(&root);
    root
}

pub(super) fn mount_state_path(deployment_id: i64) -> PathBuf {
    mount_state_base_path().join(format!("{}.json", deployment_id))
}

pub(super) fn mount_state_lock_path(deployment_id: i64) -> PathBuf {
    mount_state_base_path().join(format!("{}.lock", deployment_id))
}

pub(super) fn rclone_cache_base_path() -> PathBuf {
    let root = wacht_agent_base_path().join("cache").join("rclone");
    let _ = std::fs::create_dir_all(&root);
    root
}

pub(super) async fn prepare_mount_path(mount_root: &Path) -> Result<(), AppError> {
    if fs::try_exists(mount_root).await.unwrap_or(false) {
        let _ = fs::remove_dir_all(mount_root).await;
    }

    fs::create_dir_all(mount_root).await.map_err(|e| {
        AppError::Internal(format!(
            "Failed to prepare deployment mount root '{}': {}",
            mount_root.display(),
            e
        ))
    })?;

    Ok(())
}

pub(super) async fn cleanup_mount_artifacts(
    deployment_id: i64,
    mount_root: &Path,
) -> Result<(), AppError> {
    if is_mount_live(mount_root).await? {
        unmount_path(mount_root).await?;
    }

    match fs::remove_dir_all(mount_root).await {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(AppError::Internal(format!(
                "Failed to remove deployment mount root '{}': {}",
                mount_root.display(),
                error
            )));
        }
    }

    remove_mount_state(deployment_id).await?;
    Ok(())
}

pub(super) async fn cleanup_orphan_mount_directory(mount_root: &Path) -> Result<(), AppError> {
    if is_mount_live(mount_root).await? {
        unmount_path(mount_root).await?;
    }

    match fs::remove_dir_all(mount_root).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Internal(format!(
            "Failed to remove orphan deployment mount root '{}': {}",
            mount_root.display(),
            error
        ))),
    }
}

pub(super) async fn load_mount_state(deployment_id: i64) -> Result<Option<MountState>, AppError> {
    let path = mount_state_path(deployment_id);
    let exists = fs::try_exists(&path).await.unwrap_or(false);
    if !exists {
        return Ok(None);
    }

    let bytes = fs::read(&path).await.map_err(|e| {
        AppError::Internal(format!(
            "Failed to read deployment mount state '{}': {}",
            path.display(),
            e
        ))
    })?;

    let raw_value = serde_json::from_slice::<serde_json::Value>(&bytes).map_err(|e| {
        AppError::Internal(format!(
            "Failed to parse deployment mount state '{}': {}",
            path.display(),
            e
        ))
    })?;
    let contains_legacy_remote_path = raw_value.get("remote_path").is_some();

    let state = serde_json::from_value::<MountState>(raw_value).map_err(|e| {
        AppError::Internal(format!(
            "Failed to parse deployment mount state '{}': {}",
            path.display(),
            e
        ))
    })?;

    if contains_legacy_remote_path {
        save_mount_state(deployment_id, &state).await?;
    }
    Ok(Some(state))
}

pub(super) async fn save_mount_state(
    deployment_id: i64,
    state: &MountState,
) -> Result<(), AppError> {
    let path = mount_state_path(deployment_id);
    let parent = path.parent().ok_or_else(|| {
        AppError::Internal("Failed to determine deployment mount state directory".to_string())
    })?;
    fs::create_dir_all(parent).await.map_err(|e| {
        AppError::Internal(format!(
            "Failed to create deployment mount state directory '{}': {}",
            parent.display(),
            e
        ))
    })?;

    let bytes = serde_json::to_vec_pretty(state).map_err(|e| {
        AppError::Internal(format!("Failed to serialize deployment mount state: {}", e))
    })?;

    fs::write(&path, bytes).await.map_err(|e| {
        AppError::Internal(format!(
            "Failed to persist deployment mount state '{}': {}",
            path.display(),
            e
        ))
    })?;

    Ok(())
}

pub(super) async fn remove_mount_state(deployment_id: i64) -> Result<(), AppError> {
    let path = mount_state_path(deployment_id);
    match fs::remove_file(&path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Internal(format!(
            "Failed to remove deployment mount state '{}': {}",
            path.display(),
            error
        ))),
    }
}

pub(super) fn touch_holder(state: &mut MountState, holder_id: &str, now: u64) {
    let holder = state
        .holders
        .entry(holder_id.to_string())
        .or_insert_with(MountHolder::current);
    holder.pid = std::process::id();
    holder.active_leases += 1;
    holder.last_used_unix_ms = now;
    state.last_activity_unix_ms = now;
}

pub(super) async fn prune_stale_holders(state: &mut MountState) {
    let mut stale_holders = Vec::new();
    for (holder_id, holder) in &state.holders {
        if holder.active_leases == 0 || !process_is_alive(holder.pid).await {
            stale_holders.push(holder_id.clone());
        }
    }

    for holder_id in stale_holders {
        state.holders.remove(&holder_id);
    }
}

pub(super) async fn mount_state_lock_is_stale(path: &Path) -> bool {
    let Ok(bytes) = fs::read(path).await else {
        return true;
    };

    let Ok(owner) = serde_json::from_slice::<MountStateLockOwner>(&bytes) else {
        return true;
    };

    !process_is_alive(owner.pid).await
}

pub(super) async fn process_is_alive(pid: u32) -> bool {
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .await
        .map(|output| output.status.success())
        .unwrap_or(false)
}
