use crate::filesystem::AgentFilesystem;
use common::error::AppError;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const TASK_WORKSPACE_DIR: &str = "/task";
pub const TASK_WORKSPACE_TASK_FILE: &str = "/task/TASK.md";
pub const TASK_WORKSPACE_JOURNAL_FILE: &str = "/task/JOURNAL.md";
pub const TASK_WORKSPACE_RUNBOOK_FILE: &str = "/task/RUNBOOK.md";

pub struct TaskWorkspaceBriefInput<'a> {
    pub task_key: &'a str,
    pub title: &'a str,
    pub is_recurring: bool,
}

pub struct PreparedTaskWorkspace {
    pub journal_hash: String,
}

pub async fn prepare_task_workspace_layout_at_path(
    task_path: &Path,
    input: &TaskWorkspaceBriefInput<'_>,
) -> Result<PreparedTaskWorkspace, AppError> {
    let handoffs_path = task_path.join("handoffs");
    let artifacts_path = task_path.join("artifacts");
    let notes_path = task_path.join("notes");
    let journal_file_path = task_path.join("JOURNAL.md");
    let runbook_file_path = task_path.join("RUNBOOK.md");

    for path in [task_path, &handoffs_path, &artifacts_path, &notes_path] {
        tokio::fs::create_dir_all(path).await.map_err(|err| {
            AppError::Internal(format!(
                "Failed to prepare task workspace '{}': {}",
                path.display(),
                err
            ))
        })?;
    }

    ensure_task_journal_exists(&journal_file_path, input.task_key, input.title).await?;
    if input.is_recurring {
        ensure_task_runbook_exists(&runbook_file_path, input.task_key, input.title).await?;
    }

    Ok(PreparedTaskWorkspace {
        journal_hash: compute_file_hash_at_path(&journal_file_path).await?,
    })
}

pub async fn compute_task_journal_hash(filesystem: &AgentFilesystem) -> Result<String, AppError> {
    let bytes = filesystem
        .read_file_bytes(TASK_WORKSPACE_JOURNAL_FILE)
        .await?;
    Ok(hash_bytes(&bytes))
}

async fn ensure_task_journal_exists(
    journal_file_path: &Path,
    task_key: &str,
    title: &str,
) -> Result<(), AppError> {
    if tokio::fs::metadata(journal_file_path).await.is_ok() {
        return Ok(());
    }

    let contents = format!(
        "# Journal\n\nTask key: {}\nTask title: {}\n\nThis file is the append-only task history. Update it with durable handoff context before you finish a task stage.\n",
        task_key, title
    );

    tokio::fs::write(journal_file_path, contents)
        .await
        .map_err(|err| {
            AppError::Internal(format!(
                "Failed to write task journal file '{}': {}",
                journal_file_path.display(),
                err
            ))
        })
}

async fn ensure_task_runbook_exists(
    runbook_file_path: &Path,
    task_key: &str,
    title: &str,
) -> Result<(), AppError> {
    if tokio::fs::metadata(runbook_file_path).await.is_ok() {
        return Ok(());
    }

    let contents = format!(
        "# Runbook\n\nTask key: {}\nTask title: {}\n\nKeep this file short. Store only critical carry-forward facts:\n- key file or script paths\n- main data shape or storage location\n- reusable commands or procedures\n- non-obvious gotchas or invariants\n\nDo not put timeline history here. Use `/task/JOURNAL.md` for that.\n",
        task_key, title
    );

    tokio::fs::write(runbook_file_path, contents)
        .await
        .map_err(|err| {
            AppError::Internal(format!(
                "Failed to write task runbook file '{}': {}",
                runbook_file_path.display(),
                err
            ))
        })
}

async fn compute_file_hash_at_path(path: &Path) -> Result<String, AppError> {
    let bytes = tokio::fs::read(path).await.map_err(|err| {
        AppError::Internal(format!(
            "Failed to read task journal file '{}' for hashing: {}",
            path.display(),
            err
        ))
    })?;
    Ok(hash_bytes(&bytes))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut encoded, "{:02x}", byte);
    }
    encoded
}
