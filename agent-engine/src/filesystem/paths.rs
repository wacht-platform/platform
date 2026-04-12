use super::AgentFilesystem;
use std::path::{Path, PathBuf};

pub fn knowledge_base_mount_name(kb_id: &str, kb_name: &str) -> String {
    let mut sanitized = String::with_capacity(kb_name.len());
    let mut prev_was_separator = false;

    for ch in kb_name.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            prev_was_separator = false;
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' {
            prev_was_separator = false;
            Some(ch)
        } else if ch.is_whitespace() {
            if prev_was_separator {
                None
            } else {
                prev_was_separator = true;
                Some('_')
            }
        } else {
            None
        };

        if let Some(value) = normalized {
            sanitized.push(value);
        }
    }

    let sanitized = sanitized.trim_matches(['_', '-']).to_string();
    let sanitized = if sanitized.is_empty() {
        "knowledge_base".to_string()
    } else {
        sanitized
    };

    format!("{}_{}", sanitized, kb_id)
}

impl AgentFilesystem {
    pub fn execution_root(&self) -> PathBuf {
        self.execution_base_path.join(&self.execution_id)
    }

    pub fn persistent_uploads_path(&self) -> PathBuf {
        self.durable_root_path
            .join("persistent")
            .join(&self.thread_id)
            .join("uploads")
    }

    pub fn persistent_workspace_path(&self) -> PathBuf {
        self.durable_root_path
            .join("persistent")
            .join(&self.thread_id)
            .join("workspace")
    }

    pub fn persistent_project_root_path(&self) -> PathBuf {
        self.durable_root_path.join(&self.project_id)
    }

    pub fn persistent_task_path(&self, task_key: &str) -> PathBuf {
        self.persistent_project_root_path()
            .join("tasks")
            .join(task_key)
    }

    pub fn mounted_task_path(&self) -> PathBuf {
        self.execution_root().join("task")
    }

    pub fn mounted_project_workspace_path(&self) -> PathBuf {
        self.execution_root().join("project_workspace")
    }

    pub fn shared_kb_path(&self, kb_id: &str) -> PathBuf {
        self.durable_root_path.join("knowledge-bases").join(kb_id)
    }

    pub fn system_skills_source_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("skills")
            .join("system")
    }

    pub fn persistent_agent_skills_path_for_root(
        durable_root_path: &Path,
        agent_id: &str,
    ) -> PathBuf {
        durable_root_path
            .join("agents")
            .join(agent_id)
            .join("skills")
    }

    pub fn persistent_agent_skills_path(&self) -> PathBuf {
        Self::persistent_agent_skills_path_for_root(&self.durable_root_path, &self.agent_id)
    }

    pub fn mounted_skills_root_path(&self) -> PathBuf {
        self.execution_root().join("skills")
    }

    pub fn mounted_system_skills_path(&self) -> PathBuf {
        self.mounted_skills_root_path().join("system")
    }

    pub fn mounted_agent_skills_path(&self) -> PathBuf {
        self.mounted_skills_root_path().join("agent")
    }
}
