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
pub const EXTERNAL_TOOL_NAME_PREFIX: &str = "composio_";
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
    /// Stable tool name exposed to the LLM. `composio_gmail_send_email`.
    pub fn tool_name(&self) -> String {
        format!(
            "{}{}",
            EXTERNAL_TOOL_NAME_PREFIX,
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

pub async fn search_external_tools(
    state: &AppState,
    deployment_id: i64,
    actor_id: i64,
    query: &str,
    limit: usize,
) -> Result<Vec<ExternalToolCandidate>, AppError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let Some(settings) = load_composio_runtime_settings(state, deployment_id).await? else {
        return Ok(Vec::new());
    };

    let connected = GetActiveComposioSlugsForActorQuery::new(
        deployment_id,
        actor_id,
        settings.enabled_toolkit_slugs.clone(),
    )
    .execute_with_db(state.db_router.writer())
    .await?;
    if connected.is_empty() {
        return Ok(Vec::new());
    }

    let limit = limit.clamp(1, 25);
    let toolkits_param = connected.join(",");

    let resp = http_client()
        .get(format!("{COMPOSIO_API_BASE}/api/v3/tools"))
        .header("x-api-key", &settings.api_key)
        .query(&[
            ("search", query),
            ("toolkits", toolkits_param.as_str()),
            ("limit", &limit.to_string()),
        ])
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
            "composio tools search returned {status}: {text}"
        )));
    }

    let parsed: ComposioSearchResponse = serde_json::from_str(&text).map_err(|e| {
        AppError::Internal(format!("composio tools search parse: {e}; body: {text}"))
    })?;

    let connected_set: std::collections::HashSet<String> =
        connected.iter().cloned().collect();

    Ok(parsed
        .items
        .into_iter()
        .filter_map(|t| {
            let slug = t.slug.clone()?;
            let toolkit = t
                .toolkit
                .as_ref()
                .and_then(|tk| tk.slug.clone())
                .unwrap_or_default();
            if !connected_set.contains(&toolkit) {
                return None;
            }
            Some(ExternalToolCandidate {
                provider: EXTERNAL_PROVIDER_COMPOSIO.to_string(),
                toolkit_slug: toolkit,
                remote_tool_slug: slug,
                display_name: t.name.unwrap_or_default(),
                description: t.description.unwrap_or_default(),
                input_schema: t.input_parameters,
            })
        })
        .collect())
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
        "tool_slug": config.remote_tool_slug,
        "user_id": user_id,
        "arguments": arguments,
    });

    let resp = http_client()
        .post(format!("{COMPOSIO_API_BASE}/api/v3/tools/execute"))
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
