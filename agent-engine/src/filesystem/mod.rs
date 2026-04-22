use common::error::AppError;
use common::state::AppState;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

mod lifecycle;
mod paths;
mod skills;
mod text_io;

pub mod mounts;
pub mod sandbox;
pub mod shell;

#[allow(unused_imports)]
pub(crate) mod runtime {
    pub(crate) use super::mounts;
    pub(crate) use super::sandbox;
    pub(crate) use super::shell;
}

pub use paths::knowledge_base_mount_name;

pub(crate) type InitCell =
    tokio::sync::OnceCell<std::result::Result<mounts::DeploymentMountLease, Arc<AppError>>>;

#[derive(Clone)]
pub struct AgentFilesystem {
    execution_base_path: PathBuf,
    durable_root_path: PathBuf,
    deployment_id: i64,
    app_state: AppState,
    agent_id: String,
    project_id: String,
    thread_id: String,
    execution_id: String,
    knowledge_bases: Vec<(String, String)>,
    read_windows: Arc<RwLock<HashMap<String, Vec<ReadWindow>>>>,
    pub(crate) init_cell: Arc<InitCell>,
}

#[derive(Debug, Clone)]
pub struct ReadFileResult {
    pub content: String,
    pub total_lines: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub slice_hash: String,
}

#[derive(Debug, Clone)]
struct ReadWindow {
    start_line: usize,
    end_line: usize,
    slice_hash: String,
}

#[derive(Debug, Clone)]
pub struct WriteFileResult {
    pub lines_written: usize,
    pub total_lines: usize,
    pub partial: bool,
}

#[derive(Debug, Clone)]
pub struct EditFileResult {
    pub lines_written: usize,
    pub total_lines: usize,
    pub partial: bool,
    pub replaced_content: String,
}
