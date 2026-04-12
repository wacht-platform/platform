use super::{paths::knowledge_base_mount_name, AgentFilesystem};
use common::error::AppError;
use common::state::AppState;
use std::path::{Path, PathBuf};
use tokio::fs;

impl AgentFilesystem {
    pub async fn new(
        app_state: &AppState,
        deployment_id: &str,
        agent_id: &str,
        project_id: &str,
        thread_id: &str,
        execution_id: &str,
    ) -> Result<Self, AppError> {
        let deployment_id_num = deployment_id.parse::<i64>().map_err(|e| {
            AppError::Internal(format!(
                "Invalid deployment id '{}' for filesystem mount resolution: {}",
                deployment_id, e
            ))
        })?;
        let mount_lease = super::mounts::acquire_deployment_root(app_state, deployment_id_num).await?;
        let execution_base_path = super::mounts::detect_local_execution_base_path()
            .join(deployment_id)
            .join("executions");

        Ok(Self {
            execution_base_path,
            durable_root_path: mount_lease.root_path().to_path_buf(),
            mount_lease,
            agent_id: agent_id.to_string(),
            project_id: project_id.to_string(),
            thread_id: thread_id.to_string(),
            execution_id: execution_id.to_string(),
            read_windows: std::sync::Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        })
    }

    pub async fn initialize(&self) -> Result<(), AppError> {
        let root = self.execution_root();

        fs::create_dir_all(&root)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create execution root: {}", e)))?;

        fs::create_dir_all(root.join("scratch"))
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create scratch: {}", e)))?;

        fs::create_dir_all(root.join("knowledge"))
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create knowledge dir: {}", e)))?;
        fs::create_dir_all(self.mounted_skills_root_path())
            .await
            .map_err(|e| AppError::Internal(format!("Failed to create skills dir: {}", e)))?;

        let persistent_uploads = self.persistent_uploads_path();
        fs::create_dir_all(&persistent_uploads).await.map_err(|e| {
            AppError::Internal(format!("Failed to create persistent uploads: {}", e))
        })?;

        let uploads_link = root.join("uploads");
        if !uploads_link.exists() {
            fs::symlink(&persistent_uploads, &uploads_link)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to symlink uploads: {}", e)))?;
        }

        let persistent_workspace = self.persistent_workspace_path();
        fs::create_dir_all(&persistent_workspace)
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to create persistent workspace: {}", e))
            })?;

        let workspace_link = root.join("workspace");
        if !workspace_link.exists() {
            fs::symlink(&persistent_workspace, &workspace_link)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to symlink workspace: {}", e)))?;
        }

        let persistent_project_root = self.persistent_project_root_path();
        fs::create_dir_all(&persistent_project_root)
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to create project workspace root: {}", e))
            })?;

        let project_workspace_link = self.mounted_project_workspace_path();
        if !project_workspace_link.exists() {
            fs::symlink(&persistent_project_root, &project_workspace_link)
                .await
                .map_err(|e| {
                    AppError::Internal(format!("Failed to symlink project workspace: {}", e))
                })?;
        }

        let persistent_agent_skills = self.persistent_agent_skills_path();
        fs::create_dir_all(&persistent_agent_skills)
            .await
            .map_err(|e| {
                AppError::Internal(format!("Failed to create persistent agent skills: {}", e))
            })?;
        self.replace_symlink_target(&persistent_agent_skills, &self.mounted_agent_skills_path())
            .await?;

        let system_skills = Self::system_skills_source_path();
        if system_skills.exists() {
            self.replace_symlink_target(&system_skills, &self.mounted_system_skills_path())
                .await?;
        } else {
            fs::create_dir_all(self.mounted_system_skills_path())
                .await
                .map_err(|e| {
                    AppError::Internal(format!("Failed to create mounted system skills dir: {}", e))
                })?;
        }

        Ok(())
    }

    async fn replace_symlink_target(&self, source: &Path, target: &Path) -> Result<(), AppError> {
        if let Ok(metadata) = fs::symlink_metadata(target).await {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                fs::remove_dir_all(target).await.map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to remove existing directory '{}': {}",
                        target.display(),
                        e
                    ))
                })?;
            } else {
                fs::remove_file(target).await.map_err(|e| {
                    AppError::Internal(format!(
                        "Failed to remove existing path '{}': {}",
                        target.display(),
                        e
                    ))
                })?;
            }
        }

        fs::symlink(source, target).await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to symlink '{}' -> '{}': {}",
                target.display(),
                source.display(),
                e
            ))
        })
    }

    pub async fn link_knowledge_base(&self, kb_id: &str, kb_name: &str) -> Result<(), AppError> {
        let source = self.shared_kb_path(kb_id);
        let target = self
            .execution_root()
            .join("knowledge")
            .join(knowledge_base_mount_name(kb_id, kb_name));

        if !source.exists() {
            fs::create_dir_all(&source)
                .await
                .map_err(|e| AppError::Internal(format!("Failed to create KB directory: {}", e)))?;
        }

        if target.exists() {
            let metadata = fs::symlink_metadata(&target).await.ok();
            if let Some(m) = metadata {
                if m.is_symlink() {
                    fs::remove_file(&target).await.ok();
                }
            }
        }

        fs::symlink(&source, &target)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to link KB: {}", e)))?;

        Ok(())
    }

    pub async fn mount_task_workspace(&self, task_key: &str) -> Result<PathBuf, AppError> {
        let persistent_task = self.persistent_task_path(task_key);
        fs::create_dir_all(&persistent_task).await.map_err(|e| {
            AppError::Internal(format!("Failed to create persistent task workspace: {}", e))
        })?;

        let task_link = self.mounted_task_path();
        if task_link.exists() {
            let metadata = fs::symlink_metadata(&task_link).await.ok();
            if let Some(metadata) = metadata {
                if metadata.is_dir() && !metadata.file_type().is_symlink() {
                    fs::remove_dir_all(&task_link).await.map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to remove existing task mount directory: {}",
                            e
                        ))
                    })?;
                } else {
                    fs::remove_file(&task_link).await.map_err(|e| {
                        AppError::Internal(format!(
                            "Failed to remove existing task mount link: {}",
                            e
                        ))
                    })?;
                }
            }
        }

        fs::symlink(&persistent_task, &task_link)
            .await
            .map_err(|e| AppError::Internal(format!("Failed to mount task workspace: {}", e)))?;

        Ok(persistent_task)
    }

    pub async fn cleanup(&self) -> Result<(), AppError> {
        let root = self.execution_root();
        if root.exists() {
            fs::remove_dir_all(&root).await.map_err(|e| {
                AppError::Internal(format!("Failed to cleanup execution root: {}", e))
            })?;
        }
        self.mount_lease.release().await?;
        Ok(())
    }
}
