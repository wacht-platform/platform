use crate::consumer::TaskError;
use common::state::AppState;
use dto::json::nats::{ApiKeyRateLimitSyncPayload, RateLimitSchemeSyncPayload};
use queries::rate_limit_scheme::GetRateLimitSchemeQuery;
use queries::{
    Query,
    api_key::{SyncApiKeyRateLimitsForAppQuery, SyncApiKeyRateLimitsForSchemeQuery},
};
use serde_json::Value;
use tracing::info;

const BATCH_SIZE: i64 = 10_000;

async fn load_rules_json(
    app_state: &AppState,
    deployment_id: i64,
    scheme_slug: &str,
) -> Result<Value, TaskError> {
    let scheme = GetRateLimitSchemeQuery::new(deployment_id, scheme_slug.to_string())
        .execute(app_state)
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to load rate limit scheme: {}", e)))?;

    let rules = scheme.map(|s| s.rules).unwrap_or_default();
    serde_json::to_value(&rules)
        .map_err(|e| TaskError::Permanent(format!("Failed to serialize rate limit rules: {}", e)))
}

pub async fn sync_rate_limits_for_app(
    payload: ApiKeyRateLimitSyncPayload,
    app_state: &AppState,
) -> Result<String, TaskError> {
    let rules_json = if let Some(ref slug) = payload.rate_limit_scheme_slug {
        load_rules_json(app_state, payload.deployment_id, slug).await?
    } else {
        serde_json::json!([])
    };

    let mut total_updated = 0i64;
    let mut last_id = 0i64;

    loop {
        let updated: Vec<i64> = SyncApiKeyRateLimitsForAppQuery::new(
            payload.deployment_id,
            payload.app_id,
            last_id,
            BATCH_SIZE,
            rules_json.clone(),
            payload.rate_limit_scheme_slug.clone(),
        )
        .execute(app_state)
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to update api keys: {}", e)))?;

        if updated.is_empty() {
            break;
        }

        total_updated += updated.len() as i64;
        if let Some(max_id) = updated.iter().copied().max() {
            last_id = max_id;
        } else {
            break;
        }
    }

    info!(
        "Synced rate limits for app {} (deployment {}), updated {} keys",
        payload.app_id, payload.deployment_id, total_updated
    );

    Ok(format!("Updated {} keys", total_updated))
}

pub async fn sync_rate_limits_for_scheme(
    payload: RateLimitSchemeSyncPayload,
    app_state: &AppState,
) -> Result<String, TaskError> {
    let rules_json =
        load_rules_json(app_state, payload.deployment_id, &payload.scheme_slug).await?;

    let mut total_updated = 0i64;
    let mut last_id = 0i64;

    loop {
        let updated: Vec<i64> = SyncApiKeyRateLimitsForSchemeQuery::new(
            payload.deployment_id,
            payload.scheme_slug.clone(),
            last_id,
            BATCH_SIZE,
            rules_json.clone(),
        )
        .execute(app_state)
        .await
        .map_err(|e| TaskError::Permanent(format!("Failed to update api keys: {}", e)))?;

        if updated.is_empty() {
            break;
        }

        total_updated += updated.len() as i64;
        if let Some(max_id) = updated.iter().copied().max() {
            last_id = max_id;
        } else {
            break;
        }
    }

    info!(
        "Synced rate limits for scheme '{}' (deployment {}), updated {} keys",
        payload.scheme_slug, payload.deployment_id, total_updated
    );

    Ok(format!("Updated {} keys", total_updated))
}
