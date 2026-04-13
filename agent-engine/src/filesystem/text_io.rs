use super::{AgentFilesystem, EditFileResult, ReadFileResult, WriteFileResult};
use common::error::AppError;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

impl AgentFilesystem {
    pub async fn save_upload(&self, filename: &str, data: &[u8]) -> Result<String, AppError> {
        let uploads_dir = self.persistent_uploads_path();
        fs::create_dir_all(&uploads_dir).await.ok();

        let file_path = uploads_dir.join(filename);
        fs::write(&file_path, data)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to save upload: {}", e)))?;

        Ok(format!("/uploads/{}", filename))
    }

    pub async fn read_file_bytes(&self, path: &str) -> Result<Vec<u8>, AppError> {
        let full_path = self.resolve_path(path)?;
        fs::read(&full_path)
            .await
            .map_err(|e| AppError::NotFound(format!("Failed to read {}: {}", path, e)))
    }

    pub async fn read_file(
        &self,
        path: &str,
        start_line: Option<usize>,
        end_line: Option<usize>,
    ) -> Result<ReadFileResult, AppError> {
        let full_path = self.resolve_path(path)?;
        let content = fs::read_to_string(&full_path)
            .await
            .map_err(|e| AppError::NotFound(format!("Failed to read {}: {}", path, e)))?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = start_line.unwrap_or(1).saturating_sub(1);
        let end = end_line.unwrap_or(total_lines).min(total_lines);

        let selected_lines: Vec<String> = lines
            .iter()
            .enumerate()
            .skip(start)
            .take(end.saturating_sub(start))
            .map(|(i, s)| Self::format_numbered_line(i + 1, s))
            .collect();
        let raw_slice = lines
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");

        let slice_hash = Self::slice_hash(&raw_slice);
        self.record_read_window(path, start + 1, end, slice_hash.clone());

        Ok(ReadFileResult {
            content: selected_lines.join("\n"),
            total_lines,
            start_line: start + 1,
            end_line: end,
            slice_hash,
        })
    }

    pub async fn write_file(
        &self,
        path: &str,
        content: &str,
        append: bool,
    ) -> Result<WriteFileResult, AppError> {
        let full_path = self.resolve_path(path)?;

        let writable_prefixes = ["memory/", "workspace/", "scratch/", "task/"];
        let clean = path.trim_start_matches('/');
        if !writable_prefixes.iter().any(|p| clean.starts_with(p)) {
            return Err(AppError::Forbidden(
                "Can only write to memory/, workspace/, scratch/, task/".to_string(),
            ));
        }

        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await.ok();
        }

        if append {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full_path)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to open {}: {}", path, e)))?;

            file.write_all(content.as_bytes())
                .await
                .map_err(|e| AppError::Internal(format!("Failed to append {}: {}", path, e)))?;
            let _ = file.sync_all().await;

            self.clear_read_windows(path);

            let final_content = fs::read_to_string(&full_path).await.unwrap_or_default();

            return Ok(WriteFileResult {
                lines_written: content.lines().count(),
                total_lines: final_content.lines().count(),
                partial: false,
            });
        }

        fs::write(&full_path, content)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write {}: {}", path, e)))?;

        if let Ok(file) = fs::File::open(&full_path).await {
            let _ = file.sync_all().await;
        }

        self.clear_read_windows(path);

        Ok(WriteFileResult {
            lines_written: content.lines().count(),
            total_lines: content.lines().count(),
            partial: false,
        })
    }

    pub async fn edit_file(
        &self,
        path: &str,
        new_content: &str,
        live_slice_hash: Option<&str>,
        dangerously_skip_slice_comparison: bool,
        start_line: usize,
        end_line: usize,
    ) -> Result<EditFileResult, AppError> {
        let full_path = self.resolve_path(path)?;

        let writable_prefixes = ["memory/", "workspace/", "scratch/", "task/"];
        let clean = path.trim_start_matches('/');
        if !writable_prefixes.iter().any(|p| clean.starts_with(p)) {
            return Err(AppError::Forbidden(
                "Can only write to memory/, workspace/, scratch/, task/".to_string(),
            ));
        }

        if !dangerously_skip_slice_comparison
            && !self.has_covering_read_window(path, start_line, end_line.max(start_line))
        {
            return Err(AppError::BadRequest(
                "Must read the target range before edit_file. Use read_file(path, start_line, end_line) first.".to_string(),
            ));
        }

        let existing = fs::read_to_string(&full_path).await.unwrap_or_default();
        let lines: Vec<String> = existing.lines().map(|s| s.to_string()).collect();

        if start_line == 0 || end_line < start_line {
            return Err(AppError::BadRequest(
                "edit_file requires a valid inclusive line range.".to_string(),
            ));
        }

        if lines.is_empty() || start_line > lines.len() || end_line > lines.len() {
            return Err(AppError::BadRequest(
                "edit_file range must stay within the current file. Re-read the exact range before editing.".to_string(),
            ));
        }

        let start = start_line.saturating_sub(1);
        let end = end_line.max(start_line).min(lines.len());
        let replaced_content = lines
            .iter()
            .skip(start)
            .take(end.saturating_sub(start))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");

        if !dangerously_skip_slice_comparison {
            let live_slice_hash = live_slice_hash.ok_or_else(|| {
                AppError::BadRequest(
                    "edit_file requires live_slice_hash unless dangerously_skip_slice_comparison is true.".to_string(),
                )
            })?;

            if !self.has_covering_read_window_with_live_hash(
                path,
                start_line,
                end_line.max(start_line),
                live_slice_hash,
                &lines,
            ) {
                return Err(AppError::BadRequest(
                    "edit_file live_slice_hash does not match any current covering read window for this edit range. Re-read the file or exact target range before editing.".to_string(),
                ));
            }
        }

        let new_lines: Vec<String> = new_content.lines().map(|s| s.to_string()).collect();

        let before: Vec<String> = lines.iter().take(start).cloned().collect();
        let after: Vec<String> = lines.iter().skip(end).cloned().collect();

        let mut result = before;
        result.extend(new_lines);
        result.extend(after);

        let final_content = result.join("\n");
        fs::write(&full_path, &final_content)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write {}: {}", path, e)))?;

        if let Ok(file) = fs::File::open(&full_path).await {
            let _ = file.sync_all().await;
        }

        self.clear_read_windows(path);

        Ok(EditFileResult {
            lines_written: new_content.lines().count(),
            total_lines: result.len(),
            partial: true,
            replaced_content,
        })
    }

    pub async fn list_dir(&self, path: &str) -> Result<Vec<String>, AppError> {
        let full_path = self.resolve_path(path)?;

        if !full_path.exists() {
            return Err(AppError::NotFound(format!("Directory not found: {}", path)));
        }

        let mut entries = fs::read_dir(&full_path)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read directory: {}", e)))?;

        let mut files = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to read entry: {}", e)))?
        {
            if let Some(name) = entry.file_name().to_str() {
                let metadata = entry.metadata().await.ok();
                let suffix = if metadata.map(|m| m.is_dir()).unwrap_or(false) {
                    "/"
                } else {
                    ""
                };
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
            return Err(AppError::BadRequest(
                "Path traversal not allowed".to_string(),
            ));
        }

        let clean = path.trim_start_matches('/');
        Ok(self.execution_root().join(clean))
    }

    pub fn resolve_path_public(&self, path: &str) -> Result<PathBuf, AppError> {
        self.resolve_path(path)
    }

    fn clear_read_windows(&self, path: &str) {
        let tracked_path = Self::tracked_path(path);
        if let Ok(mut read_windows) = self.read_windows.write() {
            read_windows.remove(&tracked_path);
        }
    }

    fn record_read_window(
        &self,
        path: &str,
        start_line: usize,
        end_line: usize,
        slice_hash: String,
    ) {
        let tracked_path = Self::tracked_path(path);
        if let Ok(mut read_windows) = self.read_windows.write() {
            let entry = read_windows.entry(tracked_path).or_default();
            entry.push(super::ReadWindow {
                start_line,
                end_line,
                slice_hash,
            });
            if entry.len() > 12 {
                let overflow = entry.len() - 12;
                entry.drain(0..overflow);
            }
        }
    }

    fn has_covering_read_window(&self, path: &str, start_line: usize, end_line: usize) -> bool {
        let tracked_path = Self::tracked_path(path);
        self.read_windows
            .read()
            .ok()
            .and_then(|read_windows| {
                read_windows.get(&tracked_path).map(|windows| {
                    windows.iter().any(|window| {
                        window.start_line <= start_line && window.end_line >= end_line
                    })
                })
            })
            .unwrap_or(false)
    }

    fn has_covering_read_window_with_live_hash(
        &self,
        path: &str,
        start_line: usize,
        end_line: usize,
        live_slice_hash: &str,
        lines: &[String],
    ) -> bool {
        let tracked_path = Self::tracked_path(path);
        self.read_windows
            .read()
            .ok()
            .and_then(|read_windows| {
                read_windows.get(&tracked_path).map(|windows| {
                    windows.iter().any(|window| {
                        if !(window.start_line <= start_line
                            && window.end_line >= end_line
                            && window.slice_hash == live_slice_hash)
                        {
                            return false;
                        }

                        let window_start = window.start_line.saturating_sub(1);
                        let window_end = window.end_line.min(lines.len());
                        if window_start >= window_end {
                            return false;
                        }

                        let current_window_slice = lines
                            .iter()
                            .skip(window_start)
                            .take(window_end.saturating_sub(window_start))
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n");
                        Self::slice_hash(&current_window_slice) == live_slice_hash
                    })
                })
            })
            .unwrap_or(false)
    }

    fn format_numbered_line(line_number: usize, line: &str) -> String {
        format!("{}: {}", line_number, line)
    }

    fn tracked_path(path: &str) -> String {
        path.trim_start_matches('/').to_string()
    }

    fn slice_hash(value: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(value.as_bytes());
        let digest = hasher.finalize();
        let mut encoded = String::with_capacity(digest.len() * 2);
        for byte in digest {
            encoded.push_str(&format!("{:02x}", byte));
        }
        encoded
    }
}
