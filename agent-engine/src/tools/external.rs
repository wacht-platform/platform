//! External-provider tool adapters (Composio today; Arcade/Pipedream later).
//!
//! External tools are *virtual*: they live only in the agent runtime context
//! and are never persisted to the DB. `search_external_tools` queries the
//! provider's own tool-search API filtered by the deployment's enabled apps
//! and the actor's active connections, then returns candidate tool defs that
//! the search meta-tool surfaces to the LLM.

use common::error::AppError;
use common::state::AppState;
use models::{
    AiTool, AiToolConfiguration, AiToolType, ComposioEnabledApp, VirtualToolConfiguration,
};
use queries::composio::{GetActiveComposioSlugsForActorQuery, GetComposioSettingsQuery};
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub const EXTERNAL_PROVIDER_COMPOSIO: &str = "composio";
pub const VIRTUAL_TOOL_NAME_PREFIX: &str = "v_";
const COMPOSIO_API_BASE: &str = "https://backend.composio.dev";

fn http_client() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(20))
            .build()
            .expect("failed to build reqwest client")
    })
}

/// A provider-native tool discovered by `search`. Converted into an `AiTool`
/// (with `AiToolConfiguration::External`) before being handed to the runtime.
#[derive(Debug, Clone)]
pub struct ExternalToolCandidate {
    pub provider: String,
    pub toolkit_slug: String,
    pub remote_tool_slug: String,
    pub display_name: String,
    pub description: String,
    pub input_schema: Option<Value>,
}

impl ExternalToolCandidate {
    pub fn tool_name(&self) -> String {
        format!(
            "{}{}_{}_{}",
            VIRTUAL_TOOL_NAME_PREFIX,
            self.provider.to_lowercase(),
            self.toolkit_slug.to_lowercase(),
            self.remote_tool_slug.to_lowercase()
        )
    }

    pub fn into_ai_tool(self, synthetic_id: i64) -> AiTool {
        let name = self.tool_name();
        let now = chrono::Utc::now();
        AiTool {
            id: synthetic_id,
            created_at: now,
            updated_at: now,
            name,
            description: Some(self.description),
            tool_type: AiToolType::Virtual,
            deployment_id: 0,
            requires_user_approval: false,
            configuration: AiToolConfiguration::Virtual(VirtualToolConfiguration {
                provider: self.provider,
                toolkit_slug: self.toolkit_slug,
                remote_tool_slug: self.remote_tool_slug,
                input_schema: self.input_schema,
            }),
        }
    }
}

/// Derive a deterministic negative i64 id for a virtual external tool so it
/// can live in collections keyed by `i64` without clashing with real DB rows
/// (which are positive). Mirrors the pattern used for virtual MCP tools.
pub fn synthetic_tool_id(provider: &str, toolkit: &str, tool: &str) -> i64 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(provider.as_bytes());
    hasher.update(b":");
    hasher.update(toolkit.as_bytes());
    hasher.update(b":");
    hasher.update(tool.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    let raw = i64::from_be_bytes(bytes);
    if raw == i64::MIN {
        -1
    } else {
        -raw.abs()
    }
}

// ---------------------------------------------------------------------------
// Composio settings resolution (mirrors platform/src/application/composio.rs)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct ComposioRuntimeSettings {
    api_key: String,
    enabled_toolkit_slugs: Vec<String>,
}

async fn load_composio_runtime_settings(
    state: &AppState,
    deployment_id: i64,
) -> Result<Option<ComposioRuntimeSettings>, AppError> {
    let row = GetComposioSettingsQuery::new(deployment_id)
        .execute_with_db(state.db_router.writer())
        .await?;

    let Some(row) = row else { return Ok(None) };
    if !row.enabled {
        return Ok(None);
    }

    let api_key = if row.use_platform_key {
        match std::env::var("COMPOSIO_PLATFORM_API_KEY") {
            Ok(v) if !v.trim().is_empty() => v,
            _ => return Ok(None),
        }
    } else {
        let Some(enc) = row.api_key else {
            return Ok(None);
        };
        if enc.trim().is_empty() {
            return Ok(None);
        }
        state
            .encryption_service
            .decrypt(&enc)
            .map_err(|e| AppError::Internal(format!("composio key decrypt: {e}")))?
    };

    let apps: Vec<ComposioEnabledApp> =
        serde_json::from_value(row.enabled_apps).unwrap_or_default();
    let enabled_toolkit_slugs = apps.into_iter().map(|a| a.slug).collect();

    Ok(Some(ComposioRuntimeSettings {
        api_key,
        enabled_toolkit_slugs,
    }))
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ComposioSearchResponse {
    #[serde(default, alias = "tools", alias = "data")]
    items: Vec<ComposioTool>,
}

#[derive(Debug, Deserialize)]
struct ComposioTool {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_parameters: Option<Value>,
    #[serde(default)]
    toolkit: Option<ComposioToolToolkit>,
}

#[derive(Debug, Deserialize)]
struct ComposioToolToolkit {
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalSearchMode {
    Keyword,
    Browse,
}

pub struct ExternalSearchOptions<'a> {
    pub mode: ExternalSearchMode,
    pub query: Option<&'a str>,
    pub apps: Option<&'a [String]>,
    pub limit: usize,
}

pub async fn search_external_tools(
    state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    options: ExternalSearchOptions<'_>,
) -> Result<Vec<ExternalToolCandidate>, AppError> {
    let query = options.query.map(|q| q.trim()).filter(|q| !q.is_empty());

    if matches!(options.mode, ExternalSearchMode::Keyword) && query.is_none() {
        tracing::warn!(deployment_id, actor_id, "composio search skipped: empty query in keyword mode");
        return Ok(Vec::new());
    }

    let Some(settings) = load_composio_runtime_settings(state, deployment_id).await? else {
        tracing::warn!(
            deployment_id,
            actor_id,
            "composio search skipped: composio not enabled for deployment (or api key missing)"
        );
        return Ok(Vec::new());
    };

    let candidate_apps: Vec<String> = match options.apps {
        Some(requested) if !requested.is_empty() => requested
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .filter(|s| settings.enabled_toolkit_slugs.iter().any(|enabled| enabled == s))
            .collect(),
        _ => settings.enabled_toolkit_slugs.clone(),
    };

    if candidate_apps.is_empty() {
        tracing::warn!(
            deployment_id,
            actor_id,
            requested_apps = ?options.apps,
            enabled_apps = ?settings.enabled_toolkit_slugs,
            "composio search skipped: no overlap between requested apps and enabled apps"
        );
        return Ok(Vec::new());
    }

    let connected = GetActiveComposioSlugsForActorQuery::new(
        deployment_id,
        actor_id,
        candidate_apps.clone(),
    )
    .execute_with_db(state.db_router.writer())
    .await?;
    if connected.is_empty() {
        tracing::warn!(
            deployment_id,
            actor_id,
            candidate_apps = ?candidate_apps,
            "composio search skipped: actor has no active connections to any candidate app"
        );
        return Ok(Vec::new());
    }

    tracing::info!(
        deployment_id,
        actor_id,
        connected_apps = ?connected,
        mode = ?options.mode,
        query = ?query,
        "composio search: querying Composio API"
    );

    let limit = options.limit.clamp(1, 25);
    let limit_str = limit.to_string();
    let mode = options.mode;
    let query_owned = query.map(|q| q.to_string());

    let per_toolkit_fetches = connected.iter().map(|toolkit| {
        let api_key = settings.api_key.clone();
        let toolkit = toolkit.clone();
        let query = query_owned.clone();
        let limit_str = limit_str.clone();
        async move {
            let mut params: Vec<(&str, String)> = vec![
                ("toolkit_slug", toolkit.clone()),
                ("toolkit_versions", "latest".to_string()),
                ("limit", limit_str.clone()),
            ];
            match mode {
                ExternalSearchMode::Keyword => {
                    if let Some(q) = query.as_ref() {
                        params.push(("query", q.clone()));
                    }
                }
                ExternalSearchMode::Browse => {}
            }
            let resp = http_client()
                .get(format!("{COMPOSIO_API_BASE}/api/v3/tools"))
                .header("x-api-key", &api_key)
                .query(&params)
                .send()
                .await
                .map_err(|e| AppError::Internal(format!("composio tools search: {e}")))?;
            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| AppError::Internal(format!("composio search body: {e}")))?;
            if !status.is_success() {
                return Err(AppError::Internal(format!(
                    "composio tools search ({toolkit}) returned {status}: {text}"
                )));
            }
            let parsed: ComposioSearchResponse = serde_json::from_str(&text).map_err(|e| {
                AppError::Internal(format!("composio tools search parse: {e}; body: {text}"))
            })?;
            Ok::<_, AppError>((toolkit, parsed.items))
        }
    });

    let batches = futures::future::join_all(per_toolkit_fetches).await;

    let mut raw_count = 0usize;
    let mut candidates: Vec<ExternalToolCandidate> = Vec::new();
    for result in batches {
        let (toolkit, items) = result?;
        raw_count += items.len();
        for t in items {
            let Some(slug) = t.slug else { continue };
            candidates.push(ExternalToolCandidate {
                provider: EXTERNAL_PROVIDER_COMPOSIO.to_string(),
                toolkit_slug: t
                    .toolkit
                    .as_ref()
                    .and_then(|tk| tk.slug.clone())
                    .unwrap_or_else(|| toolkit.clone()),
                remote_tool_slug: slug,
                display_name: t.name.unwrap_or_default(),
                description: t.description.unwrap_or_default(),
                input_schema: t.input_parameters,
            });
        }
    }

    if candidates.is_empty() {
        tracing::warn!(
            deployment_id,
            actor_id,
            query = ?query_owned,
            mode = ?mode,
            raw_count,
            connected_apps = ?connected,
            "composio search: API returned no matches"
        );
    } else {
        tracing::info!(
            deployment_id,
            actor_id,
            query = ?query_owned,
            mode = ?mode,
            raw_count,
            kept = candidates.len(),
            "composio search: candidates resolved"
        );
    }

    Ok(candidates)
}

// ---------------------------------------------------------------------------
// Execute
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ComposioExecuteResponse {
    #[serde(default, alias = "success")]
    successful: Option<bool>,
    #[serde(default, alias = "output", alias = "result")]
    data: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

pub async fn execute_external_tool(
    state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    config: &VirtualToolConfiguration,
    arguments: &Value,
) -> Result<Value, AppError> {
    if config.provider != EXTERNAL_PROVIDER_COMPOSIO {
        return Err(AppError::Validation(format!(
            "unsupported external provider: {}",
            config.provider
        )));
    }

    let Some(settings) = load_composio_runtime_settings(state, deployment_id).await? else {
        return Err(AppError::Validation(
            "Composio is not configured for this deployment".to_string(),
        ));
    };

    let user_id = format!("actor_{actor_id}");
    let body = json!({
        "user_id": user_id,
        "arguments": arguments,
        "version": "latest",
    });

    let resp = http_client()
        .post(format!(
            "{COMPOSIO_API_BASE}/api/v3/tools/execute/{}",
            config.remote_tool_slug
        ))
        .header("x-api-key", &settings.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("composio execute: {e}")))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("composio execute body: {e}")))?;

    if !status.is_success() {
        return Err(AppError::Internal(format!(
            "composio execute returned {status}: {text}"
        )));
    }

    let parsed: ComposioExecuteResponse = serde_json::from_str(&text).map_err(|e| {
        AppError::Internal(format!("composio execute parse: {e}; body: {text}"))
    })?;

    if let Some(false) = parsed.successful {
        let detail = parsed
            .error
            .as_ref()
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_else(|| "unknown error".to_string());
        return Err(AppError::BadRequest(format!(
            "composio tool failed: {detail}"
        )));
    }

    Ok(parsed.data.unwrap_or(Value::Null))
}
