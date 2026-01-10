use std::path::PathBuf;
use chrono::{Local, Utc};
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use common::error::AppError;

const RETENTION_DAYS: i64 = 15;

#[allow(dead_code)]
pub struct TeamsActivityLogger {
    deployment_id: String,
    agent_id: String,
    context_group: String,
    base_path: PathBuf,
}

impl TeamsActivityLogger {
    pub fn new(deployment_id: &str, agent_id: &str, context_group: &str) -> Self {
        // Use /mnt/wacht-agents/{deployment_id}/teams-activity/{context_group}/{agent_id}
        let base_path = PathBuf::from("/mnt/wacht-agents")
            .join(deployment_id)
            .join("teams-activity")
            .join(context_group)
            .join(agent_id);

        Self {
            deployment_id: deployment_id.to_string(),
            agent_id: agent_id.to_string(),
            context_group: context_group.to_string(),
            base_path,
        }
    }

    fn get_today_filename(&self) -> String {
        let now = Local::now();
        format!("{}.log", now.format("%Y-%m-%d"))
    }

    fn get_log_file_path(&self, filename: &str) -> PathBuf {
        self.base_path.join(filename)
    }

    pub async fn ensure_directory(&self) -> Result<(), AppError> {
        if !self.base_path.exists() {
            fs::create_dir_all(&self.base_path).await.map_err(|e| {
                AppError::Internal(format!("Failed to create teams activity directory: {}", e))
            })?;
        }
        Ok(())
    }

    pub async fn append_entry(&self, entry_type: &str, content: &str) -> Result<(), AppError> {
        self.ensure_directory().await?;

        let filename = self.get_today_filename();
        let file_path = self.get_log_file_path(&filename);
        let now = Local::now();
        let timestamp = now.format("%H:%M:%S");

        let log_line = format!("[{}] {}: {}\n", timestamp, entry_type, content);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to open log file: {}", e)))?;

        file.write_all(log_line.as_bytes())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to write to log file: {}", e)))?;

        Ok(())
    }

    pub async fn load_recent_logs(&self) -> Result<String, AppError> {
        if !self.base_path.exists() {
            return Ok(String::new());
        }

        let mut entries = fs::read_dir(&self.base_path).await.map_err(|e| {
            AppError::Internal(format!("Failed to read log directory: {}", e))
        })?;

        let mut log_files = Vec::new();
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext == "log" {
                    if let Some(file_namestr) = path.file_name().and_then(|n| n.to_str()) {
                         log_files.push(file_namestr.to_string());
                    }
                }
            }
        }

        // Sort reverse (newest first)
        log_files.sort_by(|a, b| b.cmp(a));
        
        // Take retention days
        let recent_files: Vec<String> = log_files.into_iter().take(RETENTION_DAYS as usize).collect();
        
        // Read contents (processing oldest to newest for the context string)
        let mut full_log = String::new();
        
        // Iterate REVERSE (oldest first) to build chronological context
        for filename in recent_files.iter().rev() {
            let path = self.get_log_file_path(filename);
            if let Ok(content) = fs::read_to_string(&path).await {
                full_log.push_str(&format!("=== Date: {} ===\n", filename.replace(".log", "")));
                full_log.push_str(&content);
                full_log.push_str("\n");
            }
        }

        Ok(full_log)
    }

    pub async fn cleanup_old_logs(&self) -> Result<usize, AppError> {
        if !self.base_path.exists() {
            return Ok(0);
        }

        let mut entries = fs::read_dir(&self.base_path).await.map_err(|e| {
            AppError::Internal(format!("Failed to read log directory: {}", e))
        })?;

        let cutoff_date = Utc::now() - chrono::Duration::days(RETENTION_DAYS);
        let cutoff_str = cutoff_date.format("%Y-%m-%d").to_string(); // Simple string compare works for YYYY-MM-DD
        
        let mut deleted_count = 0;

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                if filename.ends_with(".log") {
                    let date_part = filename.replace(".log", "");
                    // Using string comparison for dates
                    if date_part < cutoff_str {
                         if let Err(e) = fs::remove_file(&path).await {
                             tracing::warn!("Failed to delete old log file {}: {}", filename, e);
                         } else {
                             deleted_count += 1;
                         }
                    }
                }
            }
        }

        Ok(deleted_count)
    }
}
