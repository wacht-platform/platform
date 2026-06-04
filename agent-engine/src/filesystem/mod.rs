use common::state::AppState;
use std::collections::HashSet;
use std::sync::{Arc, RwLock};

mod lifecycle;
mod paths;
mod text_io;

pub mod shell;

pub use paths::knowledge_base_mount_name;

#[derive(Clone)]
pub struct AgentFilesystem {
    pub(crate) deployment_id: i64,
    pub(crate) app_state: AppState,
    pub(crate) thread_id: String,
    read_paths: Arc<RwLock<HashSet<String>>>,
    pub(crate) sandbox_handle: Arc<dyn crate::sandbox::SandboxHandle>,
}

#[derive(Debug, Clone)]
pub struct ReadFileResult {
    pub content: String,
    pub total_lines: usize,
    pub total_chars: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub start_char: Option<usize>,
    pub end_char: Option<usize>,
    pub slice_hash: String,
}

#[derive(Debug, Clone)]
pub struct WriteFileResult {
    pub lines_written: usize,
    pub total_lines: usize,
    pub partial: bool,
}

#[derive(Debug, Clone)]
pub struct EditFileResult {
    pub replacements: usize,
    pub total_lines: usize,
}
