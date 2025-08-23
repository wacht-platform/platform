// Console-specific API key management functions
// These functions use the SDK to call backend API endpoints

use axum::extract::{Json, Path};
use axum::http::StatusCode;
use wacht::api::api_keys;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use dto::json::api_key::{
    ApiKeyStats, ApiKeyStatus, CreateApiKeyRequest, ListApiKeysResponse, RevokeApiKeyRequest,
};
use models::{
    api_key::{ApiKey, ApiKeyApp, ApiKeyWithSecret},
};

// Get API key status for a deployment
pub async fn get_api_key_status(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyStatus> {
    let app_name = deployment_id.to_string();

    // Try to get the app using SDK
    let apps = api_keys::list_api_key_apps(Some(true))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let app = apps.into_iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(&app_name))
        .and_then(|a| serde_json::from_value::<ApiKeyApp>(a).ok());

    let keys = if let Some(ref _app) = app {
        let keys_json = api_keys::list_api_keys(&app_name, Some(true))
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        
        let keys: Vec<models::api_key::ApiKey> = keys_json.into_iter()
            .filter_map(|k| serde_json::from_value(k).ok())
            .collect();
        Some(keys)
    } else {
        None
    };

    Ok(ApiKeyStatus {
        is_activated: app.is_some(),
        app,
        keys,
    }
    .into())
}

// Activate API keys for a deployment
pub async fn activate_api_keys(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyApp> {
    let app_name = deployment_id.to_string();

    // Check if already exists using SDK
    let apps = api_keys::list_api_key_apps(Some(true))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let existing = apps.into_iter()
        .any(|a| a.get("name").and_then(|n| n.as_str()) == Some(&app_name));

    if existing {
        return Err((
            StatusCode::BAD_REQUEST,
            "API keys already activated for this deployment",
        )
            .into());
    }

    // Create API key app using SDK
    let request = api_keys::CreateApiKeyAppRequest {
        name: app_name,
        description: Some(format!("API keys for deployment {}", deployment_id)),
        rate_limit_per_minute: Some(60),
        rate_limit_per_hour: Some(1000),
        rate_limit_per_day: Some(10000),
    };

    let app_json = api_keys::create_api_key_app(request)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let app: ApiKeyApp = serde_json::from_value(app_json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(app.into())
}

// Deactivate API keys for a deployment
pub async fn deactivate_api_keys(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    let app_name = deployment_id.to_string();

    // Update app to deactivate using SDK
    let request = api_keys::UpdateApiKeyAppRequest {
        name: None,
        description: None,
        is_active: Some(false),
        rate_limit_per_minute: None,
        rate_limit_per_hour: None,
        rate_limit_per_day: None,
    };

    api_keys::update_api_key_app(&app_name, request)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(().into())
}

// Get API key statistics
pub async fn get_api_key_stats(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyStats> {
    let app_name = deployment_id.to_string();

    // Get keys using SDK
    let keys_json = api_keys::list_api_keys(&app_name, Some(true))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    let keys: Vec<ApiKey> = keys_json.into_iter()
        .filter_map(|k| serde_json::from_value(k).ok())
        .collect();

    let total_keys = keys.len() as i64;
    let active_keys = keys.iter().filter(|k| k.is_active).count() as i64;
    let revoked_keys = keys.iter().filter(|k| !k.is_active).count() as i64;

    // Calculate keys used in last 24 hours
    let now = chrono::Utc::now();
    let twenty_four_hours_ago = now - chrono::Duration::hours(24);
    let keys_used_24h = keys
        .iter()
        .filter(|k| {
            k.last_used_at
                .map(|last_used| last_used > twenty_four_hours_ago)
                .unwrap_or(false)
        })
        .count() as i64;

    Ok(ApiKeyStats {
        total_keys,
        active_keys,
        revoked_keys,
        keys_used_24h,
    }
    .into())
}

// List API keys
pub async fn list_api_keys(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListApiKeysResponse> {
    let app_name = deployment_id.to_string();

    // Get keys using SDK
    let keys_json = api_keys::list_api_keys(&app_name, Some(true))
        .await
        .map_err(|e| {
            if e.to_string().contains("404") || e.to_string().contains("Not Found") {
                (
                    StatusCode::NOT_FOUND,
                    "API key app not found. Please activate API keys first.".to_string(),
                )
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        })?;
    
    let keys: Vec<ApiKey> = keys_json.into_iter()
        .filter_map(|k| serde_json::from_value(k).ok())
        .collect();

    Ok(ListApiKeysResponse { keys }.into())
}

// Create an API key
pub async fn create_api_key(
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let app_name = deployment_id.to_string();

    // Always use default read-only scopes for console-created API keys
    let permissions = models::api_key_permissions::ApiKeyScopeHelper::scopes_to_strings(
        &models::api_key_permissions::ApiKeyScope::default_scopes()
    );

    // Create API key using SDK
    let sdk_request = api_keys::CreateApiKeyRequest {
        name: request.name,
        permissions: Some(permissions),
        expires_at: request.expires_at.map(|dt| dt.to_rfc3339()),
        metadata: request.metadata,
    };

    let key = api_keys::create_api_key(&app_name, sdk_request)
        .await
        .map_err(|e| {
            if e.to_string().contains("404") || e.to_string().contains("Not Found") {
                (
                    StatusCode::NOT_FOUND,
                    "API key app not found. Please activate API keys first.".to_string(),
                )
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        })?;
    
    // SDK returns the same structure as our model
    // Just need to convert the timestamps from strings to DateTime
    let api_key_data = ApiKey {
        id: key.key.id,
        app_id: key.key.app_id,
        deployment_id: key.key.deployment_id,
        name: key.key.name,
        key_prefix: key.key.key_prefix,
        key_suffix: key.key.key_suffix,
        key_hash: key.key.key_hash,
        permissions: key.key.permissions,
        metadata: key.key.metadata,
        expires_at: key.key.expires_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        last_used_at: key.key.last_used_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        is_active: key.key.is_active,
        created_at: chrono::DateTime::parse_from_rfc3339(&key.key.created_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&key.key.updated_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        revoked_at: key.key.revoked_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        revoked_reason: key.key.revoked_reason,
    };
    
    let api_key = ApiKeyWithSecret {
        key: api_key_data,
        secret: key.secret,
    };
    
    Ok(api_key.into())
}

// Revoke an API key
pub async fn revoke_api_key(
    Path((_, key_id)): Path<(i64, i64)>,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    // Revoke using SDK
    let sdk_request = api_keys::RevokeApiKeyRequest {
        key_id,
        reason: request.reason,
    };

    api_keys::revoke_api_key(sdk_request)
        .await
        .map_err(|e| {
            if e.to_string().contains("404") || e.to_string().contains("Not Found") {
                (StatusCode::NOT_FOUND, "API key not found".to_string())
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        })?;
    
    Ok(().into())
}

// Rotate an API key
pub async fn rotate_api_key(
    Path((_, key_id)): Path<(i64, i64)>,
) -> ApiResult<ApiKeyWithSecret> {
    // Rotate using SDK
    let sdk_request = api_keys::RotateApiKeyRequest {
        key_id,
    };

    let key = api_keys::rotate_api_key(sdk_request)
        .await
        .map_err(|e| {
            if e.to_string().contains("404") || e.to_string().contains("Not Found") {
                (StatusCode::NOT_FOUND, "API key not found".to_string())
            } else {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
        })?;
    
    // SDK returns the same structure as our model
    // Just need to convert the timestamps from strings to DateTime
    let api_key_data = ApiKey {
        id: key.key.id,
        app_id: key.key.app_id,
        deployment_id: key.key.deployment_id,
        name: key.key.name,
        key_prefix: key.key.key_prefix,
        key_suffix: key.key.key_suffix,
        key_hash: key.key.key_hash,
        permissions: key.key.permissions,
        metadata: key.key.metadata,
        expires_at: key.key.expires_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        last_used_at: key.key.last_used_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        is_active: key.key.is_active,
        created_at: chrono::DateTime::parse_from_rfc3339(&key.key.created_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        updated_at: chrono::DateTime::parse_from_rfc3339(&key.key.updated_at)
            .unwrap_or_else(|_| chrono::Utc::now().into())
            .with_timezone(&chrono::Utc),
        revoked_at: key.key.revoked_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&chrono::Utc)),
        revoked_reason: key.key.revoked_reason,
    };
    
    let api_key = ApiKeyWithSecret {
        key: api_key_data,
        secret: key.secret,
    };
    
    Ok(api_key.into())
}
