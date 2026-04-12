use super::*;

pub(super) async fn is_mount_live(mount_root: &Path) -> Result<bool, AppError> {
    if !fs::try_exists(mount_root).await.unwrap_or(false) {
        return Ok(false);
    }

    let output = Command::new("mount")
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to inspect active mounts: {}", e)))?;

    if !output.status.success() {
        return Ok(false);
    }

    let mount_root_display = mount_root.display().to_string();
    let needle = format!(" on {} ", mount_root_display);
    let suffix = format!(" on {}", mount_root_display);
    let stdout = String::from_utf8_lossy(&output.stdout);

    Ok(stdout
        .lines()
        .any(|line| line.contains(&needle) || line.ends_with(&suffix)))
}

pub(super) async fn mount_with_rclone(
    mount_root: &Path,
    deployment_id: i64,
    storage: &ResolvedDeploymentStorage,
) -> Result<(), AppError> {
    ensure_rclone_mount_available().await?;

    let remote = deployment_remote_mount_info(storage, deployment_id)?;
    let cache_dir = rclone_cache_base_path();
    fs::create_dir_all(&cache_dir).await.map_err(|e| {
        AppError::Internal(format!(
            "Failed to create rclone cache directory '{}': {}",
            cache_dir.display(),
            e
        ))
    })?;

    let output = Command::new("rclone")
        .arg("mount")
        .arg("--daemon")
        .arg("--daemon-timeout")
        .arg("30s")
        .arg("--uid")
        .arg(current_process_uid().to_string())
        .arg("--gid")
        .arg(current_process_gid().to_string())
        .arg("--cache-dir")
        .arg(&cache_dir)
        .arg("--no-modtime")
        .arg("--vfs-fast-fingerprint")
        .arg("--dir-cache-time")
        .arg("30m")
        .arg("--vfs-cache-mode")
        .arg("writes")
        .arg("--vfs-read-chunk-streams")
        .arg("16")
        .arg("--vfs-read-chunk-size")
        .arg("4M")
        .arg("--transfers")
        .arg("8")
        .arg(&remote.command_arg)
        .arg(mount_root)
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to start rclone mount: {}", e)))?;

    if !output.status.success() {
        let stderr = redact_sensitive_output(
            &String::from_utf8_lossy(&output.stderr),
            &remote.sensitive_values,
        );
        let stdout = redact_sensitive_output(
            &String::from_utf8_lossy(&output.stdout),
            &remote.sensitive_values,
        );
        return Err(AppError::Internal(format!(
            "rclone mount failed for deployment {} at {}: {}{}{}",
            deployment_id,
            remote.redacted_display,
            stderr.trim(),
            if stderr.trim().is_empty() || stdout.trim().is_empty() {
                ""
            } else {
                " | "
            },
            stdout.trim()
        )));
    }

    let mut wait_ms = RCLONE_MOUNT_READY_INITIAL_WAIT_MS;
    let mut mount_ready = false;
    for _attempt in 1..=RCLONE_MOUNT_READY_MAX_ATTEMPTS {
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
        if is_mount_live(mount_root).await? {
            mount_ready = true;
            break;
        }

        wait_ms = wait_ms.saturating_mul(2);
    }

    if !mount_ready {
        return Err(AppError::Internal(format!(
            "rclone mount finished but mount '{}' is not active",
            mount_root.display()
        )));
    }

    Ok(())
}

pub(super) fn deployment_remote_mount_info(
    storage: &ResolvedDeploymentStorage,
    deployment_id: i64,
) -> Result<DeploymentRemoteMountInfo, AppError> {
    let endpoint = storage.endpoint.as_deref().ok_or_else(|| {
        AppError::Internal("S3 deployment storage is missing an endpoint".to_string())
    })?;
    let access_key_id = storage.access_key_id.as_deref().ok_or_else(|| {
        AppError::Internal("S3 deployment storage is missing an access key id".to_string())
    })?;
    let secret_access_key = storage.secret_access_key.as_deref().ok_or_else(|| {
        AppError::Internal("S3 deployment storage is missing a secret access key".to_string())
    })?;

    let mut command_arg = format!(
        ":s3,provider=Other,env_auth=false,access_key_id={},secret_access_key={},endpoint={},region={},force_path_style={}:{}",
        quoted_connection_string_value(access_key_id),
        quoted_connection_string_value(secret_access_key),
        quoted_connection_string_value(endpoint),
        quoted_connection_string_value(&storage.region),
        if storage.force_path_style { "true" } else { "false" },
        storage.bucket(),
    );
    let mut bucket_path = storage.bucket().to_string();

    if let Some(prefix) = storage.root_prefix.as_deref() {
        let trimmed = prefix.trim_matches('/');
        if !trimmed.is_empty() {
            command_arg.push('/');
            command_arg.push_str(trimmed);
            bucket_path.push('/');
            bucket_path.push_str(trimmed);
        }
    }

    command_arg.push('/');
    command_arg.push_str(&deployment_id.to_string());
    bucket_path.push('/');
    bucket_path.push_str(&deployment_id.to_string());

    Ok(DeploymentRemoteMountInfo {
        redacted_display: format!(
            "s3://{} (endpoint={}, region={}, force_path_style={}, credentials=redacted)",
            bucket_path, endpoint, storage.region, storage.force_path_style
        ),
        identity_fingerprint: sha256_hex(&command_arg),
        sensitive_values: vec![
            access_key_id.to_string(),
            secret_access_key.to_string(),
            command_arg.clone(),
        ],
        command_arg,
    })
}

pub(super) fn quoted_connection_string_value(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

pub(super) fn redact_sensitive_output(output: &str, sensitive_values: &[String]) -> String {
    let mut redacted = output.to_string();
    for value in sensitive_values {
        if !value.is_empty() {
            redacted = redacted.replace(value, "[REDACTED]");
        }
    }
    redacted
}

pub(super) fn sha256_hex(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push_str(&format!("{:02x}", byte));
    }
    encoded
}

pub(super) fn current_mount_holder_id() -> String {
    WORKER_MOUNT_HOLDER_ID
        .get_or_init(|| format!("{}-{}", std::process::id(), unix_time_ms()))
        .clone()
}

pub(super) fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn remaining_idle_duration(last_activity_unix_ms: u64) -> Duration {
    let idle_timeout_ms = DEPLOYMENT_MOUNT_IDLE_TIMEOUT_SECS * 1000;
    let elapsed_ms = unix_time_ms().saturating_sub(last_activity_unix_ms);
    Duration::from_millis(idle_timeout_ms.saturating_sub(elapsed_ms))
}

pub(super) fn idle_timeout_elapsed(last_activity_unix_ms: u64) -> bool {
    remaining_idle_duration(last_activity_unix_ms).is_zero()
}

#[cfg(unix)]
pub(super) fn current_process_uid() -> u32 {
    unsafe { libc::geteuid() }
}

#[cfg(not(unix))]
fn current_process_uid() -> u32 {
    0
}

#[cfg(unix)]
pub(super) fn current_process_gid() -> u32 {
    unsafe { libc::getegid() }
}

#[cfg(not(unix))]
fn current_process_gid() -> u32 {
    0
}

pub(super) async fn ensure_rclone_mount_available() -> Result<(), AppError> {
    let output = Command::new("rclone")
        .arg("mount")
        .arg("--help")
        .output()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to execute rclone: {}", e)))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(AppError::Internal(
            "rclone mount is required for deployment storage mounts but is not available"
                .to_string(),
        ))
    }
}

pub(super) async fn unmount_path(mount_root: &Path) -> Result<(), AppError> {
    if !fs::try_exists(mount_root).await.unwrap_or(false) {
        return Ok(());
    }

    if !is_mount_live(mount_root).await? {
        return Ok(());
    }

    let candidates: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("umount", &[]), ("umount", &["-f"])]
    } else {
        &[
            ("umount", &[]),
            ("fusermount", &["-u"]),
            ("fusermount3", &["-u"]),
        ]
    };

    for (program, args) in candidates {
        let output = Command::new(program)
            .args(*args)
            .arg(mount_root)
            .output()
            .await;
        match output {
            Ok(result) if result.status.success() => break,
            Ok(_) => continue,
            Err(_) => continue,
        }
    }

    if is_mount_live(mount_root).await? {
        return Err(AppError::Internal(format!(
            "Failed to unmount deployment storage mount '{}'",
            mount_root.display()
        )));
    }

    Ok(())
}

pub(super) async fn cleanup_stale_mount_lock_files() -> Result<(), AppError> {
    let mut entries = fs::read_dir(mount_state_base_path())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read mount state directory: {}", e)))?;

    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        AppError::Internal(format!("Failed to iterate mount state directory: {}", e))
    })? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("lock") {
            continue;
        }

        if mount_state_lock_is_stale(&path).await {
            match fs::remove_file(&path).await {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(AppError::Internal(format!(
                        "Failed to remove stale deployment mount lock '{}': {}",
                        path.display(),
                        error
                    )));
                }
            }
        }
    }

    Ok(())
}

pub(super) async fn cleanup_startup_mount_states() -> Result<HashSet<i64>, AppError> {
    let mut retained = HashSet::new();
    let mut entries = fs::read_dir(mount_state_base_path())
        .await
        .map_err(|e| AppError::Internal(format!("Failed to read mount state directory: {}", e)))?;

    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        AppError::Internal(format!("Failed to iterate mount state directory: {}", e))
    })? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }

        let Some(deployment_id) = parse_deployment_id_from_state_path(&path) else {
            let _ = fs::remove_file(&path).await;
            continue;
        };

        let _state_lock = MountStateLock::acquire(deployment_id).await?;
        let Some(mut mount_state) = load_mount_state(deployment_id).await? else {
            continue;
        };

        prune_stale_holders(&mut mount_state).await;
        let total_active_leases = mount_state.total_active_leases();

        if total_active_leases == 0 && idle_timeout_elapsed(mount_state.last_activity_unix_ms) {
            let mount_root = PathBuf::from(&mount_state.mount_root);
            match cleanup_mount_artifacts(deployment_id, &mount_root).await {
                Ok(()) => {}
                Err(_error) => {
                    retained.insert(deployment_id);
                    save_mount_state(deployment_id, &mount_state).await?;
                }
            }
            continue;
        }

        save_mount_state(deployment_id, &mount_state).await?;
        retained.insert(deployment_id);
    }

    Ok(retained)
}

pub(super) async fn cleanup_orphan_mount_directories(
    retained_deployments: &HashSet<i64>,
) -> Result<(), AppError> {
    let mut entries = fs::read_dir(deployment_mount_base_path())
        .await
        .map_err(|e| {
            AppError::Internal(format!("Failed to read deployment mount directory: {}", e))
        })?;

    while let Some(entry) = entries.next_entry().await.map_err(|e| {
        AppError::Internal(format!(
            "Failed to iterate deployment mount directory: {}",
            e
        ))
    })? {
        let path = entry.path();
        let file_type = entry.file_type().await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to inspect deployment mount directory entry '{}': {}",
                path.display(),
                e
            ))
        })?;
        if !file_type.is_dir() {
            continue;
        }

        let Some(deployment_id) = parse_deployment_id_from_mount_path(&path) else {
            continue;
        };

        if retained_deployments.contains(&deployment_id) {
            continue;
        }

        match cleanup_orphan_mount_directory(&path).await {
            Ok(()) => {}
            Err(error) => {
                let _ = error;
            }
        }
    }

    Ok(())
}

pub(super) fn parse_deployment_id_from_state_path(path: &Path) -> Option<i64> {
    path.file_stem()?.to_str()?.parse::<i64>().ok()
}

pub(super) fn parse_deployment_id_from_mount_path(path: &Path) -> Option<i64> {
    path.file_name()?.to_str()?.parse::<i64>().ok()
}
