use super::AgentFilesystem;
use common::error::AppError;
use dto::json::SkillPromptItem;
use std::path::Path;
use tokio::fs;

impl AgentFilesystem {
    pub async fn list_skill_prompt_items(
        &self,
    ) -> Result<(Vec<SkillPromptItem>, Vec<SkillPromptItem>), AppError> {
        self.ensure_initialized().await?;
        let system_skills = Self::discover_skill_prompt_items(
            &Self::system_skills_source_path(),
            "/skills/system",
            "system",
        )
        .await?;
        let agent_skills = Self::discover_skill_prompt_items(
            &self.persistent_agent_skills_path(),
            "/skills/agent",
            "agent",
        )
        .await?;
        Ok((system_skills, agent_skills))
    }

    pub async fn discover_skill_prompt_items(
        root: &Path,
        mount_prefix: &str,
        source: &str,
    ) -> Result<Vec<SkillPromptItem>, AppError> {
        if !root.exists() {
            return Ok(Vec::new());
        }

        let mut entries = fs::read_dir(root).await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to read skills root '{}': {}",
                root.display(),
                e
            ))
        })?;
        let mut items = Vec::new();

        while let Some(entry) = entries.next_entry().await.map_err(|e| {
            AppError::Internal(format!(
                "Failed to read entry in '{}': {}",
                root.display(),
                e
            ))
        })? {
            let file_type = entry.file_type().await.map_err(|e| {
                AppError::Internal(format!(
                    "Failed to inspect skill entry '{}': {}",
                    entry.path().display(),
                    e
                ))
            })?;
            if !file_type.is_dir() {
                continue;
            }

            let slug = entry.file_name().to_string_lossy().to_string();
            if slug.starts_with('.') {
                continue;
            }
            let skill_md = entry.path().join("SKILL.md");
            if !skill_md.exists() {
                continue;
            }

            let file_contents = fs::read_to_string(&skill_md).await.ok();
            let (name, description) = Self::parse_skill_summary(file_contents.as_deref(), &slug);
            items.push(SkillPromptItem {
                slug: name,
                mount_path: format!("{}/{}", mount_prefix.trim_end_matches('/'), slug),
                description,
                source: source.to_string(),
            });
        }

        items.sort_by(|a, b| a.slug.cmp(&b.slug));
        Ok(items)
    }

    fn parse_skill_summary(contents: Option<&str>, slug: &str) -> (String, Option<String>) {
        let Some(contents) = contents else {
            return (slug.to_string(), None);
        };

        let mut name = slug.to_string();
        let mut description = None;

        for line in contents.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with("# ") && name == slug {
                name = trimmed.trim_start_matches("# ").trim().to_string();
                continue;
            }

            if !trimmed.starts_with('#') {
                description = Some(trimmed.to_string());
                break;
            }
        }

        (name, description)
    }
}
