use serde::Deserialize;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;
use std::path::PathBuf;
use tracing::warn;

use crate::consumer::TaskError;

#[derive(Debug, Deserialize)]
pub struct TeamsActivityLogTask {
    pub deployment_id: String,
    pub context_group: String,
    pub timestamp: String,
    pub direction: String,
    pub user_name: String,
    #[allow(dead_code)]
    pub user_id: Option<String>,
    pub message: String,
    #[allow(dead_code)]
    pub conversation_type: String,
    pub context_title: Option<String>,
    pub attachments: Option<Vec<AttachmentMetadata>>,
}

#[derive(Debug, Deserialize)]
pub struct AttachmentMetadata {
    #[serde(rename = "type")]
    pub attachment_type: String,
    pub name: String,
}

pub async fn write_teams_activity_log(task: TeamsActivityLogTask) -> Result<String, TaskError> {
    let base_path = PathBuf::from("/mnt/wacht-agents")
        .join(&task.deployment_id)
        .join("teams-activity")
        .join(&task.context_group);

    if let Err(e) = fs::create_dir_all(&base_path).await {
        warn!("Failed to create teams activity directory: {}", e);
        return Err(TaskError::Permanent(format!("Failed to create directory: {}", e)));
    }

    let date = task.timestamp.split('T').next().unwrap_or("unknown");
    let filename = format!("{}.log", date);
    let file_path = base_path.join(&filename);

    let time = task.timestamp
        .split('T')
        .nth(1)
        .and_then(|t| t.split('.').next())
        .unwrap_or("00:00:00");

    let direction_symbol = if task.direction == "incoming" { "→" } else { "←" };
    
    // Format: [time] [context] → user: message
    let context_label = task.context_title
        .as_ref()
        .map(|t| format!("[{}] ", t))
        .unwrap_or_default();
    
    let mut log_line = format!(
        "[{}] {}{} {}: {}\n",
        time,
        context_label,
        direction_symbol,
        task.user_name,
        task.message
    );

    if let Some(attachments) = &task.attachments {
        if !attachments.is_empty() {
            let attachment_str: Vec<String> = attachments
                .iter()
                .map(|a| format!("{}:{}", a.attachment_type, a.name))
                .collect();
            log_line = format!(
                "[{}] {}{} {} [{}]: {}\n",
                time,
                context_label,
                direction_symbol,
                task.user_name,
                attachment_str.join(", "),
                task.message
            );
        }
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file_path)
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to open log file: {}", e)))?;

    file.write_all(log_line.as_bytes())
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to write log: {}", e)))?;

    Ok(format!("Logged to {}", file_path.display()))
}
