use crate::filesystem::AgentFilesystem;
use common::error::AppError;
use sha2::{Digest, Sha256};

pub const TASK_WORKSPACE_DIR: &str = "/task";
pub const TASK_WORKSPACE_TASK_FILE: &str = "/task/TASK.md";
pub const TASK_WORKSPACE_JOURNAL_FILE: &str = "/task/JOURNAL.md";
pub const TASK_WORKSPACE_RUNBOOK_FILE: &str = "/task/RUNBOOK.md";

const JOURNAL_TAIL_BYTES: usize = 16 * 1024;

pub struct TaskWorkspaceBriefInput<'a> {
    pub task_key: &'a str,
    pub title: &'a str,
    pub is_recurring: bool,
}

pub struct PreparedTaskWorkspace {
    pub journal_hash: String,
}

async fn read_sandbox_optional(
    filesystem: &AgentFilesystem,
    path: &str,
) -> Result<Option<Vec<u8>>, AppError> {
    match filesystem.read_file_bytes(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    }
}

pub async fn prepare_task_workspace(
    filesystem: &AgentFilesystem,
    input: &TaskWorkspaceBriefInput<'_>,
) -> Result<PreparedTaskWorkspace, AppError> {
    let task_key = input.task_key;

    let journal_bytes = match read_sandbox_optional(filesystem, TASK_WORKSPACE_JOURNAL_FILE).await?
    {
        Some(bytes) => bytes,
        None => {
            let contents = format!(
                "# Journal\n\nTask key: {}\nTask title: {}\n\nThis file is the append-only task history. Update it with durable handoff context before you finish a task stage.\n",
                task_key, input.title
            );
            filesystem
                .write_file(TASK_WORKSPACE_JOURNAL_FILE, &contents, false)
                .await?;
            contents.into_bytes()
        }
    };

    if input.is_recurring
        && read_sandbox_optional(filesystem, TASK_WORKSPACE_RUNBOOK_FILE)
            .await?
            .is_none()
    {
        let contents = format!(
            "# Runbook\n\nTask key: {}\nTask title: {}\n\nKeep this file short. Store only critical carry-forward facts:\n- key file or script paths\n- main data shape or storage location\n- reusable commands or procedures\n- non-obvious gotchas or invariants\n\nDo not put timeline history here. Use `/task/JOURNAL.md` for that.\n",
            task_key, input.title
        );
        filesystem
            .write_file(TASK_WORKSPACE_RUNBOOK_FILE, &contents, false)
            .await?;
    }

    Ok(PreparedTaskWorkspace {
        journal_hash: hash_bytes(&journal_bytes),
    })
}

pub async fn compute_task_journal_hash(filesystem: &AgentFilesystem) -> Result<String, AppError> {
    let bytes = read_sandbox_optional(filesystem, TASK_WORKSPACE_JOURNAL_FILE)
        .await?
        .unwrap_or_default();
    Ok(hash_bytes(&bytes))
}

pub async fn read_task_journal_tail(
    filesystem: &AgentFilesystem,
) -> Result<Option<Vec<u8>>, AppError> {
    let Some(bytes) = read_sandbox_optional(filesystem, TASK_WORKSPACE_JOURNAL_FILE).await? else {
        return Ok(None);
    };
    if bytes.len() <= JOURNAL_TAIL_BYTES {
        Ok(Some(bytes))
    } else {
        let start = bytes.len() - JOURNAL_TAIL_BYTES;
        Ok(Some(bytes[start..].to_vec()))
    }
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
