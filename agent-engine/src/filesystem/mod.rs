use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tokio::fs;
use tokio::process::Command;
use common::error::AppError;

pub mod shell;

#[derive(Clone)]
pub struct AgentFilesystem {
    base_path: PathBuf,
    deployment_id: String,
    agent_id: String,
    context_id: String,
    execution_id: String,
    read_files: Arc<RwLock<HashSet<String>>>,
}

impl AgentFilesystem {
    pub fn new(deployment_id: &str, agent_id: &str, context_id: &str, execution_id: &str) -> Self {
        let base = "/mnt/wacht-agents".to_string();
        
        Self {
            base_path: PathBuf::from(base),
            deployment_id: deployment_id.to_string(),
            agent_id: agent_id.to_string(),
            context_id: context_id.to_string(),
            execution_id: execution_id.to_string(),
            read_files: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn execution_root(&self) -> PathBuf {
        self.base_path
            .join(&self.deployment_id)
            .join("executions")
            .join(&self.execution_id)
    }



    pub fn persistent_uploads_path(&self) -> PathBuf {
        self.base_path
            .join(&self.deployment_id)
            .join("persistent")
            .join(&self.context_id)
            .join("uploads")
    }

    pub fn shared_kb_path(&self, kb_id: &str) -> PathBuf {
        self.base_path
            .join(&self.deployment_id)
            .join("knowledge-bases")
            .join(kb_id)
    }

    pub fn teams_activity_path(&self, context_group: &str) -> PathBuf {
        // Path: /mnt/wacht-agents/{deployment_id}/teams-activity/{context_group}/{agent_id}/
        self.base_path
            .join(&self.deployment_id)
            .join("teams-activity")
            .join(context_group)
            .join(&self.agent_id)
    }

    pub async fn initialize(&self) -> Result<(), AppError> {
        let root = self.execution_root();
        
        fs::create_dir_all(&root).await.map_err(|e| {
            AppError::Internal(format!("Failed to create execution root: {}", e))
        })?;

        fs::create_dir_all(root.join("workspace")).await.map_err(|e| {
            AppError::Internal(format!("Failed to create workspace: {}", e))
        })?;

        fs::create_dir_all(root.join("scratch")).await.map_err(|e| {
            AppError::Internal(format!("Failed to create scratch: {}", e))
        })?;

        fs::create_dir_all(root.join("knowledge")).await.map_err(|e| {
            AppError::Internal(format!("Failed to create knowledge dir: {}", e))
        })?;


        let persistent_uploads = self.persistent_uploads_path();
        fs::create_dir_all(&persistent_uploads).await.map_err(|e| {
            AppError::Internal(format!("Failed to create persistent uploads: {}", e))
        })?;

        let uploads_link = root.join("uploads");
        if !uploads_link.exists() {
            fs::symlink(&persistent_uploads, &uploads_link).await.map_err(|e| {
                AppError::Internal(format!("Failed to symlink uploads: {}", e))
            })?;
        }

        Ok(())
    }

    pub async fn link_knowledge_base(&self, kb_id: &str, kb_name: &str) -> Result<(), AppError> {
        let source = self.shared_kb_path(kb_id);
        let target = self.execution_root().join("knowledge").join(kb_name);

        if !source.exists() {
            fs::create_dir_all(&source).await.map_err(|e| {
                AppError::Internal(format!("Failed to create KB directory: {}", e))
            })?;
        }

        if target.exists() {
            let metadata = fs::symlink_metadata(&target).await.ok();
            if let Some(m) = metadata {
                if m.is_symlink() {
                    fs::remove_file(&target).await.ok();
                }
            }
        }

        fs::symlink(&source, &target).await.map_err(|e| {
            AppError::Internal(format!("Failed to link KB: {}", e))
        })?;

        Ok(())
    }

    pub async fn link_teams_activity(&self, context_group: &str) -> Result<(), AppError> {
        let source = self.teams_activity_path(context_group);
        let target = self.execution_root().join("teams-activity");

        // Ensure source directory exists
        if !source.exists() {
            fs::create_dir_all(&source).await.map_err(|e| {
                AppError::Internal(format!("Failed to create teams-activity directory: {}", e))
            })?;
        }

        // Remove existing symlink if present
        if target.exists() {
            let metadata = fs::symlink_metadata(&target).await.ok();
            if let Some(m) = metadata {
                if m.is_symlink() {
                    fs::remove_file(&target).await.ok();
                }
            }
        }

        fs::symlink(&source, &target).await.map_err(|e| {
            AppError::Internal(format!("Failed to link teams-activity: {}", e))
        })?;

        Ok(())
    }

    pub async fn cleanup(&self) -> Result<(), AppError> {
        let root = self.execution_root();
        if root.exists() {
            fs::remove_dir_all(&root).await.map_err(|e| {
                AppError::Internal(format!("Failed to cleanup execution root: {}", e))
            })?;
        }
        Ok(())
    }

    pub async fn save_upload(&self, filename: &str, data: &[u8]) -> Result<String, AppError> {
        let uploads_dir = self.persistent_uploads_path();
        fs::create_dir_all(&uploads_dir).await.ok();
        
        let file_path = uploads_dir.join(filename);
        fs::write(&file_path, data).await.map_err(|e| {
            AppError::Internal(format!("Failed to save upload: {}", e))
        })?;
        
        Ok(format!("/uploads/{}", filename))
    }

    pub async fn read_file_bytes(&self, path: &str) -> Result<Vec<u8>, AppError> {
        let full_path = self.resolve_path(path)?;
        fs::read(&full_path).await.map_err(|e| {
            AppError::NotFound(format!("Failed to read {}: {}", path, e))
        })
    }

    pub async fn read_file(
        &self,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<ReadFileResult, AppError> {
        let full_path = self.resolve_path(path)?;
        let content = fs::read_to_string(&full_path).await.map_err(|e| {
            AppError::NotFound(format!("Failed to read {}: {}", path, e))
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(total_lines).min(total_lines);

        let selected_lines: Vec<String> = lines
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|s| s.to_string())
            .collect();

        if let Ok(mut read_files) = self.read_files.write() {
            read_files.insert(path.to_string());
        }

        Ok(ReadFileResult {
            content: selected_lines.join("\n"),
            total_lines,
            start_line: start + 1,
            end_line: end,
        })
    }

    pub async fn write_file(
        &self,
        path: &str,
        content: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<WriteFileResult, AppError> {
        let full_path = self.resolve_path(path)?;
        
        let writable_prefixes = ["memory/", "workspace/", "scratch/"];
        let clean = path.trim_start_matches('/');
        if !writable_prefixes.iter().any(|p| clean.starts_with(p)) {
            return Err(AppError::Forbidden("Can only write to memory/, workspace/, scratch/".to_string()));
        }

        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await.ok();
        }

        if start_line.is_some() || end_line.is_some() {
            let was_read = self.read_files.read()
                .map(|rf| rf.contains(path))
                .unwrap_or(false);
            
            if !was_read {
                return Err(AppError::BadRequest(
                    "Must read file before partial write. Use read_file first.".to_string()
                ));
            }

            let existing = fs::read_to_string(&full_path).await.unwrap_or_default();
            let lines: Vec<String> = existing.lines().map(|s| s.to_string()).collect();
            
            let start = start_line.unwrap_or(1).saturating_sub(1);
            let end = end_line.unwrap_or(lines.len()).min(lines.len());

            let new_lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
            
            let before: Vec<String> = lines.iter().take(start).cloned().collect();
            let after: Vec<String> = lines.iter().skip(end).cloned().collect();
            
            let mut result = before;
            result.extend(new_lines);
            result.extend(after);

            let final_content = result.join("\n");
            fs::write(&full_path, &final_content).await.map_err(|e| {
                AppError::Internal(format!("Failed to write {}: {}", path, e))
            })?;

            if let Ok(mut read_files) = self.read_files.write() {
                read_files.remove(path);
            }

            Ok(WriteFileResult {
                lines_written: content.lines().count(),
                total_lines: result.len(),
                partial: true,
            })
        } else {
            fs::write(&full_path, content).await.map_err(|e| {
                AppError::Internal(format!("Failed to write {}: {}", path, e))
            })?;

            if let Ok(mut read_files) = self.read_files.write() {
                read_files.remove(path);
            }

            Ok(WriteFileResult {
                lines_written: content.lines().count(),
                total_lines: content.lines().count(),
                partial: false,
            })
        }
    }

    pub async fn list_dir(&self, path: &str) -> Result<Vec<String>, AppError> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(AppError::NotFound(format!("Directory not found: {}", path)));
        }

        let mut entries = fs::read_dir(&full_path).await.map_err(|e| {
            AppError::Internal(format!("Failed to read directory: {}", e))
        })?;

        let mut files = Vec::new();
        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            AppError::Internal(format!("Failed to read entry: {}", e))
        })? {
            if let Some(name) = entry.file_name().to_str() {
                let metadata = entry.metadata().await.ok();
                let suffix = if metadata.map(|m| m.is_dir()).unwrap_or(false) { "/" } else { "" };
                files.push(format!("{}{}", name, suffix));
            }
        }

        Ok(files)
    }

    pub async fn search(&self, query: &str, path: &str) -> Result<String, AppError> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(AppError::NotFound(format!("Path not found: {}", path)));
        }

        let output = Command::new("rg")
            .args(["--json", "-i", "--follow", query])
            .current_dir(&full_path)
            .output()
            .await
            .map_err(|e| AppError::Internal(format!("Search failed: {}", e)))?;

        if !output.status.success() && output.status.code() != Some(1) {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::Internal(format!("Search error: {}", stderr)));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    fn resolve_path(&self, path: &str) -> Result<PathBuf, AppError> {
        if path.contains("..") {
            return Err(AppError::BadRequest("Path traversal not allowed".to_string()));
        }

        let clean = path.trim_start_matches('/');
        Ok(self.execution_root().join(clean))
    }

    pub fn resolve_path_public(&self, path: &str) -> Result<PathBuf, AppError> {
        self.resolve_path(path)
    }
}

#[derive(Debug, Clone)]
pub struct ReadFileResult {
    pub content: String,
    pub total_lines: usize,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone)]
pub struct WriteFileResult {
    pub lines_written: usize,
    pub total_lines: usize,
    pub partial: bool,
}
