use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::ProjectTaskBoardItemMetadata;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ProjectTaskSchedule {
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub id: i64,
    #[serde(with = "crate::utils::serde::i64_as_string")]
    pub board_id: i64,
    pub task_key: String,
    pub template_payload: serde_json::Value,
    pub mounts: serde_json::Value,
    pub status: String,
    pub schedule_kind: String,
    pub interval_seconds: Option<i64>,
    pub next_run_at: DateTime<Utc>,
    pub last_fired_at: Option<DateTime<Utc>>,
    pub overlap_policy: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleTemplatePayload {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub metadata: ProjectTaskBoardItemMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleMount {
    pub mount_path: String,
    pub s3_relative_key: String,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

pub mod mount_mode {
    pub const RW: &str = "rw";
    pub const RO: &str = "ro";
}

pub const IMPLICIT_MOUNT_PATH: &str = "/shared/";

pub mod status {
    pub const ACTIVE: &str = "active";
    pub const PAUSED: &str = "paused";
    pub const COMPLETED: &str = "completed";
}

pub mod schedule_kind {
    pub const ONCE: &str = "once";
    pub const INTERVAL: &str = "interval";
}

pub mod overlap_policy {
    pub const SKIP: &str = "skip";
    pub const PARALLEL: &str = "parallel";
}

#[derive(Debug)]
pub enum MountValidationError {
    EmptyMountPath,
    MountPathNotAbsolute,
    MountPathTraversal,
    MountPathDoubleSlash,
    MountPathTrailingWhitespace,
    MountPathTooLong,
    EmptyKey,
    KeyTraversal,
    KeyAbsolute,
    KeyDoubleSlash,
    KeyTooLong,
    InvalidMode,
    DescriptionTooLong,
}

impl std::fmt::Display for MountValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyMountPath => write!(f, "mount_path is empty"),
            Self::MountPathNotAbsolute => write!(f, "mount_path must start with '/'"),
            Self::MountPathTraversal => write!(f, "mount_path may not contain '..'"),
            Self::MountPathDoubleSlash => write!(f, "mount_path may not contain empty segments"),
            Self::MountPathTrailingWhitespace => write!(f, "mount_path has trailing whitespace"),
            Self::MountPathTooLong => write!(f, "mount_path is too long (max 128 chars)"),
            Self::EmptyKey => write!(f, "s3_relative_key is empty"),
            Self::KeyTraversal => write!(f, "s3_relative_key may not contain '..'"),
            Self::KeyAbsolute => write!(f, "s3_relative_key must be relative (no leading '/')"),
            Self::KeyDoubleSlash => write!(f, "s3_relative_key may not contain empty segments"),
            Self::KeyTooLong => write!(f, "s3_relative_key is too long (max 256 chars)"),
            Self::InvalidMode => write!(f, "mode must be 'rw' or 'ro'"),
            Self::DescriptionTooLong => write!(f, "description is too long (max 512 chars)"),
        }
    }
}

impl std::error::Error for MountValidationError {}

const MAX_MOUNT_PATH_LEN: usize = 128;
const MAX_KEY_LEN: usize = 256;
const MAX_DESCRIPTION_LEN: usize = 512;

pub fn validate_mount_path(path: &str) -> Result<(), MountValidationError> {
    if path.is_empty() {
        return Err(MountValidationError::EmptyMountPath);
    }
    if path.len() > MAX_MOUNT_PATH_LEN {
        return Err(MountValidationError::MountPathTooLong);
    }
    if path.trim() != path {
        return Err(MountValidationError::MountPathTrailingWhitespace);
    }
    if !path.starts_with('/') {
        return Err(MountValidationError::MountPathNotAbsolute);
    }
    let inner = path.strip_suffix('/').unwrap_or(path);
    let inner = inner.strip_prefix('/').unwrap_or(inner);
    if inner.is_empty() {
        return Ok(());
    }
    for seg in inner.split('/') {
        if seg.is_empty() {
            return Err(MountValidationError::MountPathDoubleSlash);
        }
        if seg == ".." {
            return Err(MountValidationError::MountPathTraversal);
        }
    }
    Ok(())
}

pub fn validate_s3_relative_key(key: &str) -> Result<(), MountValidationError> {
    if key.is_empty() {
        return Err(MountValidationError::EmptyKey);
    }
    if key.len() > MAX_KEY_LEN {
        return Err(MountValidationError::KeyTooLong);
    }
    if key.starts_with('/') {
        return Err(MountValidationError::KeyAbsolute);
    }
    let trimmed = key.strip_suffix('/').unwrap_or(key);
    for seg in trimmed.split('/') {
        if seg.is_empty() {
            return Err(MountValidationError::KeyDoubleSlash);
        }
        if seg == ".." {
            return Err(MountValidationError::KeyTraversal);
        }
    }
    Ok(())
}

pub fn validate_mount(mount: &ScheduleMount) -> Result<(), MountValidationError> {
    validate_mount_path(&mount.mount_path)?;
    validate_s3_relative_key(&mount.s3_relative_key)?;
    if mount.mode != mount_mode::RW && mount.mode != mount_mode::RO {
        return Err(MountValidationError::InvalidMode);
    }
    if let Some(desc) = mount.description.as_deref() {
        if desc.len() > MAX_DESCRIPTION_LEN {
            return Err(MountValidationError::DescriptionTooLong);
        }
    }
    Ok(())
}

pub fn implicit_mount_for_schedule(project_id: i64, schedule_id: i64) -> ScheduleMount {
    ScheduleMount {
        mount_path: IMPLICIT_MOUNT_PATH.to_string(),
        s3_relative_key: format!("{}/schedules/{}/", project_id, schedule_id),
        mode: mount_mode::RW.to_string(),
        description: Some(
            "Persistent shared workspace for this recurring task. Read state at the start \
             of each run and write any state you want to remember before you finish."
                .to_string(),
        ),
    }
}

pub fn parse_mounts(value: &serde_json::Value) -> Result<Vec<ScheduleMount>, serde_json::Error> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    serde_json::from_value(value.clone())
}
