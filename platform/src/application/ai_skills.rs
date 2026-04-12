use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::STANDARD};
use commands::{
    DeleteFromDeploymentStorageCommand, DeletePrefixFromDeploymentStorageCommand,
    ResolveDeploymentStorageCommand, WriteToDeploymentStorageCommand,
};
use common::ReadConsistency;
use common::deps;
use queries::GetAiAgentByIdQuery;
use tokio::fs;
use tokio::process::Command;

use crate::application::{AppState, response::ApiErrorResponse};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkillScope {
    System,
    Agent,
}

impl SkillScope {
    pub fn parse(value: &str) -> Result<Self, ApiErrorResponse> {
        match value {
            "system" => Ok(Self::System),
            "agent" => Ok(Self::Agent),
            _ => Err(ApiErrorResponse::bad_request(
                "scope must be either 'system' or 'agent'",
            )),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Agent => "agent",
        }
    }
}

#[derive(serde::Serialize)]
pub struct SkillTreeEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(serde::Serialize)]
pub struct SkillTreeResponse {
    pub scope: String,
    pub path: String,
    pub entries: Vec<SkillTreeEntry>,
}

#[derive(serde::Serialize)]
pub struct SkillFileResponse {
    pub scope: String,
    pub path: String,
    pub is_text: bool,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_base64: Option<String>,
}

pub struct CreateSkillBundleInput {
    pub file_name: String,
    pub file_content: Vec<u8>,
    pub replace_existing: bool,
}

fn system_skills_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../agent-engine/skills/system")
        .clean()
}

trait PathCleanExt {
    fn clean(&self) -> PathBuf;
}

impl PathCleanExt for PathBuf {
    fn clean(&self) -> PathBuf {
        let mut out = PathBuf::new();
        for component in self.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                other => out.push(other.as_os_str()),
            }
        }
        out
    }
}

fn normalize_virtual_path(path: &str) -> Result<String, ApiErrorResponse> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return Ok(String::new());
    }

    let trimmed = trimmed.trim_start_matches('/').trim_end_matches('/');
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." || part == ".." {
            return Err(ApiErrorResponse::bad_request("invalid path"));
        }
        if part.contains('\\') {
            return Err(ApiErrorResponse::bad_request("invalid path"));
        }
        parts.push(part);
    }

    Ok(parts.join("/"))
}

fn sanitize_skill_slug(value: &str) -> Result<String, ApiErrorResponse> {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_dash = false;
        } else if ch == '-' || ch == '_' || ch.is_ascii_whitespace() {
            if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
    }
    let slug = out.trim_matches('-').to_string();
    if slug.is_empty() {
        return Err(ApiErrorResponse::bad_request("invalid skill name"));
    }
    Ok(slug)
}

async fn ensure_agent_exists(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
) -> Result<(), ApiErrorResponse> {
    GetAiAgentByIdQuery::new(deployment_id, agent_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await
        .map(|_| ())
        .map_err(|_| ApiErrorResponse::not_found("Agent not found"))
}

fn deployment_skill_root(deployment_id: i64, agent_id: i64) -> String {
    format!("{}/agents/{}/skills", deployment_id, agent_id)
}

pub async fn list_skill_tree(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    scope: SkillScope,
    path: String,
) -> Result<SkillTreeResponse, ApiErrorResponse> {
    ensure_agent_exists(app_state, deployment_id, agent_id).await?;
    let normalized = normalize_virtual_path(&path)?;

    match scope {
        SkillScope::System => {
            let root = system_skills_root();
            let target = if normalized.is_empty() {
                root.clone()
            } else {
                root.join(&normalized)
            };
            if !target.starts_with(&root) {
                return Err(ApiErrorResponse::bad_request("invalid path"));
            }
            if !target.exists() {
                return Err(ApiErrorResponse::not_found("Path not found"));
            }
            if !target.is_dir() {
                return Err(ApiErrorResponse::bad_request("path is not a directory"));
            }

            let mut entries = Vec::new();
            let mut dir = fs::read_dir(&target)
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
            while let Some(entry) = dir
                .next_entry()
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
            {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with('.') {
                    continue;
                }
                let metadata = entry
                    .metadata()
                    .await
                    .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
                let entry_path = if normalized.is_empty() {
                    format!("/{}", name)
                } else {
                    format!("/{}/{}", normalized, name)
                };
                entries.push(SkillTreeEntry {
                    name,
                    path: entry_path,
                    kind: if metadata.is_dir() {
                        "directory".to_string()
                    } else {
                        "file".to_string()
                    },
                    size_bytes: metadata.is_file().then_some(metadata.len()),
                });
            }
            entries.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(SkillTreeResponse {
                scope: scope.as_str().to_string(),
                path: if normalized.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", normalized)
                },
                entries,
            })
        }
        SkillScope::Agent => {
            let deps = deps::from_app(app_state).db().enc();
            let storage = ResolveDeploymentStorageCommand::new(deployment_id)
                .execute_with_deps(&deps)
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;

            let base_relative = deployment_skill_root(deployment_id, agent_id);
            let current_relative = if normalized.is_empty() {
                base_relative.clone()
            } else {
                format!("{}/{}", base_relative, normalized)
            };
            let list_prefix = if normalized.is_empty() {
                storage.object_key(&format!("{}/", base_relative))
            } else {
                storage.object_key(&format!("{}/", current_relative))
            };

            let mut continuation = None;
            let mut files = BTreeMap::<String, u64>::new();
            let mut dirs = BTreeSet::<String>::new();

            loop {
                let mut request = storage
                    .client()
                    .list_objects_v2()
                    .bucket(storage.bucket())
                    .prefix(&list_prefix);
                if let Some(token) = continuation.as_deref() {
                    request = request.continuation_token(token);
                }
                let response = request
                    .send()
                    .await
                    .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;

                if let Some(contents) = response.contents {
                    for object in contents {
                        let Some(key) = object.key else { continue };
                        let Some(remainder) = key.strip_prefix(&list_prefix) else {
                            continue;
                        };
                        if remainder.is_empty() {
                            continue;
                        }
                        let mut segments = remainder.split('/').filter(|s| !s.is_empty());
                        let Some(first) = segments.next() else {
                            continue;
                        };
                        if segments.next().is_some() {
                            dirs.insert(first.to_string());
                        } else {
                            files.insert(first.to_string(), object.size.unwrap_or(0).max(0) as u64);
                        }
                    }
                }

                if response.is_truncated.unwrap_or(false) {
                    continuation = response.next_continuation_token;
                } else {
                    break;
                }
            }

            let mut entries = Vec::new();
            for dir_name in dirs {
                let entry_path = if normalized.is_empty() {
                    format!("/{}", dir_name)
                } else {
                    format!("/{}/{}", normalized, dir_name)
                };
                entries.push(SkillTreeEntry {
                    name: dir_name,
                    path: entry_path,
                    kind: "directory".to_string(),
                    size_bytes: None,
                });
            }
            for (file_name, size_bytes) in files {
                let entry_path = if normalized.is_empty() {
                    format!("/{}", file_name)
                } else {
                    format!("/{}/{}", normalized, file_name)
                };
                entries.push(SkillTreeEntry {
                    name: file_name,
                    path: entry_path,
                    kind: "file".to_string(),
                    size_bytes: Some(size_bytes),
                });
            }
            entries.sort_by(|a, b| a.name.cmp(&b.name));

            Ok(SkillTreeResponse {
                scope: scope.as_str().to_string(),
                path: if normalized.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{}", normalized)
                },
                entries,
            })
        }
    }
}

pub async fn read_skill_file(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    scope: SkillScope,
    path: String,
) -> Result<SkillFileResponse, ApiErrorResponse> {
    ensure_agent_exists(app_state, deployment_id, agent_id).await?;
    let normalized = normalize_virtual_path(&path)?;
    if normalized.is_empty() {
        return Err(ApiErrorResponse::bad_request("file path is required"));
    }

    match scope {
        SkillScope::System => {
            let root = system_skills_root();
            let target = root.join(&normalized);
            if !target.starts_with(&root) {
                return Err(ApiErrorResponse::bad_request("invalid path"));
            }
            let bytes = fs::read(&target)
                .await
                .map_err(|_| ApiErrorResponse::not_found("File not found"))?;
            let is_text = std::str::from_utf8(&bytes).is_ok();
            Ok(SkillFileResponse {
                scope: scope.as_str().to_string(),
                path: format!("/{}", normalized),
                is_text,
                size_bytes: bytes.len() as u64,
                content: is_text.then(|| String::from_utf8_lossy(&bytes).to_string()),
                content_base64: (!is_text).then(|| STANDARD.encode(bytes)),
            })
        }
        SkillScope::Agent => {
            let deps = deps::from_app(app_state).db().enc();
            let storage = ResolveDeploymentStorageCommand::new(deployment_id)
                .execute_with_deps(&deps)
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
            let relative = format!(
                "{}/{}",
                deployment_skill_root(deployment_id, agent_id),
                normalized
            );
            let key = storage.object_key(&relative);
            let response = storage
                .client()
                .get_object()
                .bucket(storage.bucket())
                .key(&key)
                .send()
                .await
                .map_err(|_| ApiErrorResponse::not_found("File not found"))?;
            let bytes = response
                .body
                .collect()
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
                .into_bytes()
                .to_vec();
            let is_text = std::str::from_utf8(&bytes).is_ok();
            Ok(SkillFileResponse {
                scope: scope.as_str().to_string(),
                path: format!("/{}", normalized),
                is_text,
                size_bytes: bytes.len() as u64,
                content: is_text.then(|| String::from_utf8_lossy(&bytes).to_string()),
                content_base64: (!is_text).then(|| STANDARD.encode(bytes)),
            })
        }
    }
}

pub async fn import_agent_skill_bundle(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    input: CreateSkillBundleInput,
) -> Result<SkillTreeResponse, ApiErrorResponse> {
    ensure_agent_exists(app_state, deployment_id, agent_id).await?;

    let zip_name = input.file_name.trim();
    if zip_name.is_empty() || !zip_name.to_ascii_lowercase().ends_with(".zip") {
        return Err(ApiErrorResponse::bad_request(
            "skill upload must be a zip archive",
        ));
    }
    if input.file_content.is_empty() {
        return Err(ApiErrorResponse::bad_request("zip archive is empty"));
    }

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let temp_root =
        std::env::temp_dir().join(format!("wacht-skill-import-{}-{}", agent_id, unique));
    let extract_root = temp_root.join("extract");
    fs::create_dir_all(&extract_root)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    let zip_path = temp_root.join("bundle.zip");
    fs::write(&zip_path, &input.file_content)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;

    let listing = Command::new("unzip")
        .arg("-Z1")
        .arg(&zip_path)
        .output()
        .await
        .map_err(|e| ApiErrorResponse::internal(format!("failed to inspect zip: {}", e)))?;
    if !listing.status.success() {
        let _ = fs::remove_dir_all(&temp_root).await;
        return Err(ApiErrorResponse::bad_request("invalid zip archive"));
    }

    let listing_text = String::from_utf8_lossy(&listing.stdout);
    let mut entry_names = Vec::new();
    for raw in listing_text.lines() {
        let normalized = raw.trim().replace('\\', "/");
        if normalized.is_empty() || normalized.starts_with("__MACOSX/") {
            continue;
        }
        if normalized.starts_with('/') || normalized.split('/').any(|part| part == "..") {
            let _ = fs::remove_dir_all(&temp_root).await;
            return Err(ApiErrorResponse::bad_request("zip contains invalid paths"));
        }
        entry_names.push(normalized);
    }
    if entry_names.is_empty() {
        let _ = fs::remove_dir_all(&temp_root).await;
        return Err(ApiErrorResponse::bad_request(
            "zip archive does not contain any files",
        ));
    }

    let unzip = Command::new("unzip")
        .arg("-q")
        .arg(&zip_path)
        .arg("-d")
        .arg(&extract_root)
        .output()
        .await
        .map_err(|e| ApiErrorResponse::internal(format!("failed to extract zip: {}", e)))?;
    if !unzip.status.success() {
        let _ = fs::remove_dir_all(&temp_root).await;
        return Err(ApiErrorResponse::bad_request(
            "failed to extract zip archive",
        ));
    }

    let top_entries = top_level_entries(&extract_root).await?;
    let (skill_root, slug) = if extract_root.join("SKILL.md").exists() {
        let file_stem = Path::new(zip_name)
            .file_stem()
            .and_then(OsStr::to_str)
            .unwrap_or("skill");
        (extract_root.clone(), sanitize_skill_slug(file_stem)?)
    } else if top_entries.len() == 1 {
        let only = &top_entries[0];
        let candidate = extract_root.join(only);
        if candidate.join("SKILL.md").exists() {
            (candidate, sanitize_skill_slug(only)?)
        } else {
            let _ = fs::remove_dir_all(&temp_root).await;
            return Err(ApiErrorResponse::bad_request(
                "skill bundle must contain SKILL.md at the root",
            ));
        }
    } else {
        let _ = fs::remove_dir_all(&temp_root).await;
        return Err(ApiErrorResponse::bad_request(
            "zip must contain either a single top-level skill folder or SKILL.md at the zip root",
        ));
    };

    reject_symlinks(&skill_root).await?;

    let deps = deps::from_app(app_state).db().enc();
    let storage = ResolveDeploymentStorageCommand::new(deployment_id)
        .execute_with_deps(&deps)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    let skill_prefix = format!(
        "{}/{}/",
        deployment_skill_root(deployment_id, agent_id),
        slug
    );
    let exists = storage
        .client()
        .list_objects_v2()
        .bucket(storage.bucket())
        .prefix(storage.object_key(&skill_prefix))
        .max_keys(1)
        .send()
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
        .key_count
        .unwrap_or(0)
        > 0;

    if exists {
        if input.replace_existing {
            DeletePrefixFromDeploymentStorageCommand::new(
                deployment_id,
                format!(
                    "{}/{}",
                    deployment_skill_root(deployment_id, agent_id),
                    slug
                ),
            )
            .execute_with_deps(&deps)
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
        } else {
            let _ = fs::remove_dir_all(&temp_root).await;
            return Err(ApiErrorResponse::bad_request(
                "skill already exists; set replace=true to overwrite",
            ));
        }
    }

    let mut stack = vec![skill_root.clone()];
    while let Some(dir) = stack.pop() {
        let mut reader = fs::read_dir(&dir)
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
        while let Some(entry) = reader
            .next_entry()
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
        {
            let path = entry.path();
            let metadata = entry
                .metadata()
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if !metadata.is_file() {
                continue;
            }
            let relative = path
                .strip_prefix(&skill_root)
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            let body = fs::read(&path)
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
            WriteToDeploymentStorageCommand::new(
                deployment_id,
                format!(
                    "{}/{}/{}",
                    deployment_skill_root(deployment_id, agent_id),
                    slug,
                    relative
                ),
                body,
            )
            .execute_with_deps(&deps)
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
        }
    }

    let _ = fs::remove_dir_all(&temp_root).await;
    list_skill_tree(
        app_state,
        deployment_id,
        agent_id,
        SkillScope::Agent,
        format!("/{}", slug),
    )
    .await
}

pub async fn delete_agent_skill_path(
    app_state: &AppState,
    deployment_id: i64,
    agent_id: i64,
    path: String,
) -> Result<(), ApiErrorResponse> {
    ensure_agent_exists(app_state, deployment_id, agent_id).await?;
    let normalized = normalize_virtual_path(&path)?;
    if normalized.is_empty() {
        return Err(ApiErrorResponse::bad_request("path is required"));
    }

    let deps = deps::from_app(app_state).db().enc();
    let relative = format!(
        "{}/{}",
        deployment_skill_root(deployment_id, agent_id),
        normalized
    );
    let storage = ResolveDeploymentStorageCommand::new(deployment_id)
        .execute_with_deps(&deps)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    let exact_key = storage.object_key(&relative);
    let file_exists = storage
        .client()
        .list_objects_v2()
        .bucket(storage.bucket())
        .prefix(&exact_key)
        .max_keys(2)
        .send()
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
        .contents
        .unwrap_or_default()
        .into_iter()
        .any(|obj| obj.key.as_deref() == Some(exact_key.as_str()));

    if file_exists {
        DeleteFromDeploymentStorageCommand::new(deployment_id, relative.clone())
            .execute_with_deps(&deps)
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    }

    let prefix = format!("{}/", storage.object_key(&relative));
    let dir_exists = storage
        .client()
        .list_objects_v2()
        .bucket(storage.bucket())
        .prefix(&prefix)
        .max_keys(1)
        .send()
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
        .key_count
        .unwrap_or(0)
        > 0;
    if dir_exists {
        DeletePrefixFromDeploymentStorageCommand::new(deployment_id, relative)
            .execute_with_deps(&deps)
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    }

    if !file_exists && !dir_exists {
        return Err(ApiErrorResponse::not_found("path not found"));
    }

    Ok(())
}

async fn top_level_entries(root: &Path) -> Result<Vec<String>, ApiErrorResponse> {
    let mut entries = Vec::new();
    let mut dir = fs::read_dir(root)
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
    {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == "__MACOSX" || name.starts_with('.') {
            continue;
        }
        entries.push(name);
    }
    entries.sort();
    Ok(entries)
}

async fn reject_symlinks(root: &Path) -> Result<(), ApiErrorResponse> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        let metadata = fs::symlink_metadata(&path)
            .await
            .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(ApiErrorResponse::bad_request(
                "zip archive may not contain symlinks",
            ));
        }
        if metadata.is_dir() {
            let mut dir = fs::read_dir(&path)
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?;
            while let Some(entry) = dir
                .next_entry()
                .await
                .map_err(|e| ApiErrorResponse::internal(e.to_string()))?
            {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "__MACOSX" {
                    continue;
                }
                stack.push(entry.path());
            }
        }
    }
    Ok(())
}
