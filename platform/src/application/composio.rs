use commands::composio::UpdateComposioConfigCommand;
use common::db_router::ReadConsistency;
use common::deps;
use common::error::AppError;
use models::{
    ComposioAuthConfigListResponse, ComposioAuthConfigSummary, ComposioConfigResponse,
    ComposioEnableAppAuth, ComposioEnabledApp, ComposioToolkit, ComposioToolkitAuthField,
    ComposioToolkitAuthFields, ComposioToolkitAuthMode, ComposioToolkitDetailsResponse,
    ComposioToolkitListResponse, EnableComposioAppRequest, UpdateComposioConfigRequest,
};
use queries::composio::GetComposioSettingsQuery;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::application::AppState;

const COMPOSIO_API_BASE: &str = "https://backend.composio.dev";

fn wacht_auth_config_name(deployment_id: i64, slug: &str) -> String {
    format!("wacht/{deployment_id}/{slug}")
}

fn deployment_name_prefix(deployment_id: i64) -> String {
    format!("wacht/{deployment_id}/")
}

async fn is_production_deployment(app_state: &AppState, deployment_id: i64) -> Result<bool, AppError> {
    let row = sqlx::query!(
        r#"SELECT mode FROM deployments WHERE id = $1 AND deleted_at IS NULL"#,
        deployment_id,
    )
    .fetch_optional(app_state.db_router.reader(ReadConsistency::Eventual))
    .await?;

    let mode = row.map(|r| r.mode).ok_or_else(|| {
        AppError::NotFound(format!("deployment {deployment_id} not found"))
    })?;
    Ok(mode.eq_ignore_ascii_case("production"))
}

pub async fn get_composio_config(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<ComposioConfigResponse, AppError> {
    let row = GetComposioSettingsQuery::new(deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    Ok(match row {
        Some(row) => row.into(),
        None => ComposioConfigResponse {
            enabled: false,
            use_platform_key: true,
            api_key_set: false,
            enabled_apps: Vec::new(),
        },
    })
}

pub async fn update_composio_config(
    app_state: &AppState,
    deployment_id: i64,
    updates: UpdateComposioConfigRequest,
) -> Result<ComposioConfigResponse, AppError> {
    if matches!(updates.use_platform_key, Some(true))
        && is_production_deployment(app_state, deployment_id).await?
    {
        return Err(AppError::Validation(
            "Production deployments must bring their own Composio API key.".to_string(),
        ));
    }

    let deps = deps::from_app(app_state).db().enc();
    UpdateComposioConfigCommand::new(deployment_id, updates)
        .execute_with_deps(&deps)
        .await?;

    get_composio_config(app_state, deployment_id).await
}

pub struct ListToolkitsParams {
    pub search: Option<String>,
    pub category: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

pub async fn list_toolkits(
    app_state: &AppState,
    deployment_id: i64,
    params: ListToolkitsParams,
) -> Result<ComposioToolkitListResponse, AppError> {
    let api_key = resolve_composio_api_key(app_state, deployment_id).await?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(format!("composio client: {e}")))?;

    let mut req = client
        .get(format!("{COMPOSIO_API_BASE}/api/v3/toolkits"))
        .header("x-api-key", &api_key);

    let mut query: Vec<(&str, String)> = Vec::new();
    if let Some(s) = params
        .search
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| s.chars().count() >= 3)
    {
        query.push(("search", s.to_string()));
    }
    if let Some(c) = params.category.as_ref().filter(|s| !s.trim().is_empty()) {
        query.push(("category", c.trim().to_string()));
    }
    if let Some(c) = params.cursor.as_ref().filter(|s| !s.trim().is_empty()) {
        query.push(("cursor", c.trim().to_string()));
    }
    if let Some(limit) = params.limit {
        query.push(("limit", limit.clamp(1, 100).to_string()));
    }
    if !query.is_empty() {
        req = req.query(&query);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("composio request failed: {e}")))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("composio read body: {e}")))?;

    if !status.is_success() {
        return Err(AppError::Internal(format!(
            "composio returned {status}: {body}"
        )));
    }

    let raw: RawToolkitsResponse = serde_json::from_str(&body).map_err(|e| {
        AppError::Internal(format!("composio response parse: {e}; body: {body}"))
    })?;

    Ok(ComposioToolkitListResponse {
        toolkits: raw.items.into_iter().map(Into::into).collect(),
        next_cursor: raw.next_cursor,
    })
}

async fn resolve_composio_api_key(
    app_state: &AppState,
    deployment_id: i64,
) -> Result<String, AppError> {
    let row = GetComposioSettingsQuery::new(deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;

    let (use_platform_key, encrypted_key) = match row {
        Some(r) => (r.use_platform_key, r.api_key),
        None => (true, None),
    };

    if use_platform_key {
        std::env::var("COMPOSIO_PLATFORM_API_KEY").map_err(|_| {
            AppError::Validation(
                "Platform-managed Composio key is not configured for this environment. Turn on 'Bring your own Composio key' and provide a key."
                    .to_string(),
            )
        })
    } else {
        let encrypted = encrypted_key.ok_or_else(|| {
            AppError::Validation(
                "Composio API key is not set. Enter your Composio API key to enable integrations."
                    .to_string(),
            )
        })?;
        app_state.encryption_service.decrypt(&encrypted)
    }
}

#[derive(Debug, Deserialize)]
struct RawToolkitsResponse {
    #[serde(default)]
    items: Vec<RawToolkit>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawToolkit {
    slug: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    meta: Option<RawToolkitMeta>,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    categories: Vec<RawCategory>,
    #[serde(default)]
    auth_schemes: Vec<String>,
    #[serde(default)]
    tool_count: Option<i64>,
    #[serde(default)]
    no_auth: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct RawToolkitMeta {
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    categories: Option<Vec<RawCategory>>,
    #[serde(default)]
    tools_count: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum RawCategory {
    Str(String),
    Obj {
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        slug: Option<String>,
    },
}

impl RawCategory {
    fn into_name(self) -> String {
        match self {
            RawCategory::Str(s) => s,
            RawCategory::Obj { name, slug } => name.or(slug).unwrap_or_default(),
        }
    }
}

impl From<RawToolkit> for ComposioToolkit {
    fn from(raw: RawToolkit) -> Self {
        let name = raw.name.unwrap_or_else(|| raw.slug.clone());
        let logo = raw.logo.or_else(|| raw.meta.as_ref().and_then(|m| m.logo.clone()));
        let description = raw
            .description
            .or_else(|| raw.meta.as_ref().and_then(|m| m.description.clone()));
        let raw_categories: Vec<RawCategory> = if !raw.categories.is_empty() {
            raw.categories
        } else {
            raw.meta
                .as_ref()
                .and_then(|m| m.categories.clone())
                .unwrap_or_default()
        };
        let categories: Vec<String> = raw_categories
            .into_iter()
            .map(RawCategory::into_name)
            .filter(|s| !s.is_empty())
            .collect();
        let tool_count = raw
            .tool_count
            .or_else(|| raw.meta.as_ref().and_then(|m| m.tools_count))
            .unwrap_or(0);
        let mut auth_schemes = raw.auth_schemes;
        if auth_schemes.is_empty() && raw.no_auth.unwrap_or(false) {
            auth_schemes.push("no_auth".to_string());
        }

        Self {
            slug: raw.slug,
            name,
            description,
            logo,
            categories,
            auth_schemes,
            tool_count,
        }
    }
}

pub async fn enable_app(
    app_state: &AppState,
    deployment_id: i64,
    request: EnableComposioAppRequest,
) -> Result<ComposioConfigResponse, AppError> {
    let slug = request.slug.trim().to_lowercase();
    if slug.is_empty() {
        return Err(AppError::Validation("slug is required".to_string()));
    }

    let existing = get_composio_config(app_state, deployment_id).await?;
    if existing.enabled_apps.iter().any(|app| app.slug == slug) {
        return Err(AppError::Validation(format!(
            "{slug} is already enabled for this deployment"
        )));
    }

    let api_key = resolve_composio_api_key(app_state, deployment_id).await?;
    let (auth_config_id, auth_scheme) = match &request.auth {
        ComposioEnableAppAuth::UseExisting {
            auth_config_id,
            auth_scheme,
        } => (
            auth_config_id.trim().to_string(),
            auth_scheme.as_ref().map(|s| s.to_uppercase()),
        ),
        ComposioEnableAppAuth::Custom { auth_scheme, .. } => (
            create_composio_auth_config(&api_key, deployment_id, &slug, &request.auth).await?,
            Some(auth_scheme.to_uppercase()),
        ),
        ComposioEnableAppAuth::Managed { auth_scheme, .. } => (
            create_composio_auth_config(&api_key, deployment_id, &slug, &request.auth).await?,
            auth_scheme.as_ref().map(|s| s.to_uppercase()),
        ),
    };

    let mut apps = existing.enabled_apps;
    apps.push(ComposioEnabledApp {
        slug: slug.clone(),
        auth_config_id,
        display_name: request.display_name,
        logo_url: request.logo_url,
        auth_scheme,
    });

    let deps = deps::from_app(app_state).db().enc();
    UpdateComposioConfigCommand::new(
        deployment_id,
        UpdateComposioConfigRequest {
            enabled: None,
            use_platform_key: None,
            api_key: None,
            enabled_apps: Some(apps),
        },
    )
    .execute_with_deps(&deps)
    .await?;

    get_composio_config(app_state, deployment_id).await
}

pub async fn disable_app(
    app_state: &AppState,
    deployment_id: i64,
    slug: &str,
) -> Result<ComposioConfigResponse, AppError> {
    let slug = slug.trim().to_lowercase();
    let existing = get_composio_config(app_state, deployment_id).await.inspect_err(|e| {
        tracing::error!(deployment_id, %slug, error = %e, "[CMP_DISABLE] load config failed")
    })?;

    let (mut remaining, removed): (Vec<_>, Vec<_>) = existing
        .enabled_apps
        .into_iter()
        .partition(|app| app.slug != slug);

    if removed.is_empty() {
        return Err(AppError::NotFound(format!("{slug} is not enabled")));
    }

    if let Ok(api_key) = resolve_composio_api_key(app_state, deployment_id).await {
        for app in &removed {
            if let Err(e) = delete_composio_auth_config(&api_key, &app.auth_config_id).await {
                tracing::warn!(
                    deployment_id, %slug, auth_config_id = %app.auth_config_id, error = %e,
                    "[CMP_DISABLE] composio delete failed (continuing)"
                );
            }
        }
    }

    remaining.sort_by(|a, b| a.slug.cmp(&b.slug));
    let deps = deps::from_app(app_state).db().enc();
    UpdateComposioConfigCommand::new(
        deployment_id,
        UpdateComposioConfigRequest {
            enabled: None,
            use_platform_key: None,
            api_key: None,
            enabled_apps: Some(remaining),
        },
    )
    .execute_with_deps(&deps)
    .await
    .inspect_err(|e| {
        tracing::error!(deployment_id, %slug, error = %e, "[CMP_DISABLE] DB update failed")
    })?;

    get_composio_config(app_state, deployment_id).await
}

async fn create_composio_auth_config(
    api_key: &str,
    deployment_id: i64,
    slug: &str,
    auth: &ComposioEnableAppAuth,
) -> Result<String, AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| AppError::Internal(format!("composio client: {e}")))?;

    let name = wacht_auth_config_name(deployment_id, slug);
    let body = match auth {
        ComposioEnableAppAuth::Managed { credentials, .. } => {
            let mut auth_config = serde_json::Map::new();
            auth_config.insert("type".to_string(), json!("use_composio_managed_auth"));
            auth_config.insert("name".to_string(), json!(name));
            if !credentials.is_empty() {
                auth_config.insert(
                    "credentials".to_string(),
                    serde_json::Value::Object(credentials.clone()),
                );
            }
            json!({
                "toolkit": { "slug": slug },
                "auth_config": auth_config,
            })
        }
        ComposioEnableAppAuth::Custom {
            auth_scheme,
            credentials,
        } => json!({
            "toolkit": { "slug": slug },
            "auth_config": {
                "type": "use_custom_auth",
                "authScheme": auth_scheme.to_uppercase(),
                "name": name,
                "credentials": credentials,
            },
        }),
        ComposioEnableAppAuth::UseExisting { .. } => {
            return Err(AppError::Internal(
                "UseExisting should not reach create_composio_auth_config".to_string(),
            ));
        }
    };

    let resp = client
        .post(format!("{COMPOSIO_API_BASE}/api/v3/auth_configs"))
        .header("x-api-key", api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("composio auth_configs POST: {e}")))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("composio read body: {e}")))?;

    if !status.is_success() {
        return Err(AppError::Validation(format!(
            "Composio rejected the auth config ({status}): {text}"
        )));
    }

    let parsed: CreateAuthConfigResponse = serde_json::from_str(&text).map_err(|e| {
        AppError::Internal(format!("composio auth_config parse: {e}; body: {text}"))
    })?;

    parsed
        .auth_config
        .map(|ac| ac.id)
        .or(parsed.id)
        .ok_or_else(|| {
            AppError::Internal(format!(
                "composio did not return an auth_config id: {text}"
            ))
        })
}

pub async fn get_toolkit_auth_details(
    app_state: &AppState,
    deployment_id: i64,
    slug: &str,
) -> Result<ComposioToolkitDetailsResponse, AppError> {
    let slug = slug.trim().to_lowercase();
    if slug.is_empty() {
        return Err(AppError::Validation("slug is required".to_string()));
    }

    let api_key = resolve_composio_api_key(app_state, deployment_id)
        .await
        .inspect_err(|e| {
            tracing::error!(deployment_id, %slug, error = %e, "[CMP_AUTH_DETAILS] resolve api key failed")
        })?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(format!("composio client: {e}")))?;

    let resp = client
        .get(format!("{COMPOSIO_API_BASE}/api/v3/toolkits/{slug}"))
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|e| {
            tracing::error!(deployment_id, %slug, error = %e, "[CMP_AUTH_DETAILS] http send failed");
            AppError::Internal(format!("composio toolkit details: {e}"))
        })?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("composio read body: {e}")))?;
    if !status.is_success() {
        tracing::error!(deployment_id, %slug, %status, body = %text, "[CMP_AUTH_DETAILS] upstream non-2xx");
        return Err(AppError::Internal(format!(
            "composio returned {status}: {text}"
        )));
    }

    let raw: RawToolkitDetails = serde_json::from_str(&text).map_err(|e| {
        tracing::error!(deployment_id, %slug, error = %e, body = %text, "[CMP_AUTH_DETAILS] parse failed");
        AppError::Internal(format!("composio toolkit details parse: {e}"))
    })?;

    Ok(ComposioToolkitDetailsResponse {
        slug: raw.slug,
        name: raw.name.unwrap_or_else(|| slug.clone()),
        logo: raw.meta.as_ref().and_then(|m| m.logo.clone()).or(raw.logo),
        composio_managed_auth_schemes: raw.composio_managed_auth_schemes,
        auth_modes: raw
            .auth_config_details
            .into_iter()
            .map(Into::into)
            .collect(),
    })
}

#[derive(Debug, Deserialize)]
struct RawToolkitDetails {
    slug: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    logo: Option<String>,
    #[serde(default)]
    meta: Option<RawToolkitMeta>,
    #[serde(default)]
    composio_managed_auth_schemes: Vec<String>,
    #[serde(default)]
    auth_config_details: Vec<RawAuthConfigDetail>,
}

#[derive(Debug, Deserialize)]
struct RawAuthConfigDetail {
    mode: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    fields: Option<RawAuthConfigFields>,
    #[serde(default)]
    auth_hint_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawAuthConfigFields {
    #[serde(default)]
    auth_config_creation: Option<RawAuthFieldGroup>,
    #[serde(default)]
    connected_account_initiation: Option<RawAuthFieldGroup>,
}

#[derive(Debug, Default, Deserialize)]
struct RawAuthFieldGroup {
    #[serde(default)]
    required: Vec<RawAuthField>,
    #[serde(default)]
    optional: Vec<RawAuthField>,
}

#[derive(Debug, Deserialize)]
struct RawAuthField {
    name: String,
    #[serde(rename = "displayName")]
    display_name: String,
    #[serde(rename = "type")]
    field_type: String,
    description: String,
    required: bool,
    #[serde(default)]
    default: Option<String>,
}

impl From<RawAuthField> for ComposioToolkitAuthField {
    fn from(raw: RawAuthField) -> Self {
        Self {
            name: raw.name,
            display_name: raw.display_name,
            field_type: raw.field_type,
            description: raw.description,
            required: raw.required,
            default: raw.default,
        }
    }
}

impl From<RawAuthFieldGroup> for ComposioToolkitAuthFields {
    fn from(raw: RawAuthFieldGroup) -> Self {
        Self {
            required: raw.required.into_iter().map(Into::into).collect(),
            optional: raw.optional.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<RawAuthConfigDetail> for ComposioToolkitAuthMode {
    fn from(raw: RawAuthConfigDetail) -> Self {
        let (auth_config_creation, connected_account_initiation) = match raw.fields {
            Some(f) => (
                f.auth_config_creation.map(Into::into).unwrap_or_default(),
                f.connected_account_initiation
                    .map(Into::into)
                    .unwrap_or_default(),
            ),
            None => Default::default(),
        };
        Self {
            name: raw.name.unwrap_or_else(|| raw.mode.clone()),
            mode: raw.mode,
            auth_config_creation,
            connected_account_initiation,
            auth_hint_url: raw.auth_hint_url,
        }
    }
}

async fn delete_composio_auth_config(api_key: &str, auth_config_id: &str) -> Result<(), AppError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Internal(format!("composio client: {e}")))?;

    let resp = client
        .delete(format!(
            "{COMPOSIO_API_BASE}/api/v3/auth_configs/{auth_config_id}"
        ))
        .header("x-api-key", api_key)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("composio auth_configs DELETE: {e}")))?;

    if !resp.status().is_success() && resp.status() != reqwest::StatusCode::NOT_FOUND {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "composio auth_configs DELETE {status}: {text}"
        )));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
struct CreateAuthConfigResponse {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    auth_config: Option<CreatedAuthConfigInner>,
}

#[derive(Debug, Deserialize)]
struct CreatedAuthConfigInner {
    id: String,
}

// Suppress unused Serialize import warning; reserved if future DTOs need it.
#[allow(dead_code)]
fn _marker_use_serialize<T: Serialize>(_: &T) {}

pub async fn list_toolkit_auth_configs(
    app_state: &AppState,
    deployment_id: i64,
    slug: &str,
) -> Result<ComposioAuthConfigListResponse, AppError> {
    // Only the BYO path exposes existing configs; on the platform-managed key
    // we'd leak other tenants' configs if we passed results through.
    let row = GetComposioSettingsQuery::new(deployment_id)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Eventual))
        .await?;
    let use_platform_key = row.as_ref().map(|r| r.use_platform_key).unwrap_or(true);
    if use_platform_key {
        return Ok(ComposioAuthConfigListResponse {
            auth_configs: Vec::new(),
        });
    }

    let api_key = resolve_composio_api_key(app_state, deployment_id).await?;
    let prefix = deployment_name_prefix(deployment_id);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::Internal(format!("composio client: {e}")))?;

    let resp = client
        .get(format!("{COMPOSIO_API_BASE}/api/v3/auth_configs"))
        .header("x-api-key", api_key)
        .query(&[("toolkit", slug)])
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("composio list auth_configs: {e}")))?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("composio read body: {e}")))?;
    if !status.is_success() {
        return Err(AppError::Internal(format!(
            "composio returned {status}: {text}"
        )));
    }

    let raw: RawAuthConfigsResponse = serde_json::from_str(&text).map_err(|e| {
        AppError::Internal(format!("composio auth_configs parse: {e}; body: {text}"))
    })?;

    let auth_configs: Vec<ComposioAuthConfigSummary> = raw
        .items
        .into_iter()
        .filter(|c| c.name.as_deref().map(|n| n.starts_with(&prefix)).unwrap_or(false))
        .filter_map(|c| {
            let toolkit_slug = c
                .toolkit
                .as_ref()
                .and_then(|t| t.slug.clone())
                .unwrap_or_else(|| slug.to_string());
            Some(ComposioAuthConfigSummary {
                id: c.id?,
                name: c.name.unwrap_or_default(),
                auth_scheme: c.auth_scheme,
                is_composio_managed: c.is_composio_managed.unwrap_or(false),
                toolkit_slug,
            })
        })
        .collect();

    Ok(ComposioAuthConfigListResponse { auth_configs })
}

#[derive(Debug, Deserialize)]
struct RawAuthConfigsResponse {
    #[serde(default)]
    items: Vec<RawAuthConfig>,
}

#[derive(Debug, Deserialize)]
struct RawAuthConfig {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    auth_scheme: Option<String>,
    #[serde(default)]
    is_composio_managed: Option<bool>,
    #[serde(default)]
    toolkit: Option<RawAuthConfigToolkit>,
}

#[derive(Debug, Deserialize)]
struct RawAuthConfigToolkit {
    #[serde(default)]
    slug: Option<String>,
}
