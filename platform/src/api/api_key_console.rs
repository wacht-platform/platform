// Console-specific API key management functions
// These functions use the SDK to call backend API endpoints

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;
use wacht::api::api_keys;

use crate::application::response::ApiResult;
use crate::middleware::RequireDeployment;
use common::state::AppState;
use dto::json::api_key::{
    ApiKeyStats, ApiKeyStatus, CreateApiKeyRequest, ListApiKeysResponse, RevokeApiKeyRequest,
};
use models::api_key::{ApiKey, ApiAuthApp, ApiKeyWithSecret};
use queries::Query;

// Get API key status for a deployment
pub async fn get_api_key_status(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyStatus> {
    let app_name = deployment_id.to_string();

    // Try to get the app using SDK
    let sdk_app = api_keys::get_api_auth_app(&app_name)
        .await
        .ok();

    let sdk_keys = if sdk_app.is_some() {
        api_keys::list_api_keys(&app_name, Some(true))
            .await
            .ok()
    } else {
        None
    };

    // Convert SDK types to model types
    let app = sdk_app.map(|sdk_app| ApiAuthApp {
        id: sdk_app.id.parse().unwrap_or(0),
        deployment_id: sdk_app.deployment_id.parse().unwrap_or(0),
        name: sdk_app.name,
        description: sdk_app.description,
        is_active: sdk_app.is_active,
        rate_limits: vec![],  // Console doesn't need rate limits
        created_at: sdk_app.created_at,
        updated_at: sdk_app.updated_at,
        deleted_at: None,
    });

    let keys = sdk_keys.map(|keys| {
        keys.into_iter()
            .map(|sdk_key| ApiKey {
                id: sdk_key.id.parse().unwrap_or(0),
                app_id: sdk_key.app_id.parse().unwrap_or(0),
                deployment_id: sdk_key.deployment_id.parse().unwrap_or(0),
                name: sdk_key.name,
                key_prefix: sdk_key.key_prefix,
                key_suffix: sdk_key.key_suffix,
                key_hash: String::new(),
                permissions: sdk_key.permissions,
                metadata: sdk_key.metadata,
                expires_at: sdk_key.expires_at,
                last_used_at: sdk_key.last_used_at,
                is_active: sdk_key.is_active,
                created_at: sdk_key.created_at,
                updated_at: sdk_key.updated_at,
                revoked_at: sdk_key.revoked_at,
                revoked_reason: sdk_key.revoked_reason,
            })
            .collect()
    });

    Ok(ApiKeyStatus {
        is_activated: app.is_some(),
        app,
        keys,
    }
    .into())
}

// Deactivate API keys for a deployment
pub async fn deactivate_api_keys(
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    let app_name = deployment_id.to_string();

    // Update app to deactivate using SDK
    let request = api_keys::UpdateApiAuthAppRequest {
        name: None,
        description: None,
        is_active: Some(false),
        rate_limits: None,
    };

    api_keys::update_api_auth_app(&app_name, request)
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
    let sdk_keys = api_keys::list_api_keys(&app_name, Some(true))
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let keys: Vec<ApiKey> = sdk_keys
        .into_iter()
        .map(|sdk_key| ApiKey {
            id: sdk_key.id.parse().unwrap_or(0),
            app_id: sdk_key.app_id.parse().unwrap_or(0),
            deployment_id: sdk_key.deployment_id.parse().unwrap_or(0),
            name: sdk_key.name,
            key_prefix: sdk_key.key_prefix,
            key_suffix: sdk_key.key_suffix,
            key_hash: String::new(),
            permissions: sdk_key.permissions,
            metadata: sdk_key.metadata,
            expires_at: sdk_key.expires_at,
            last_used_at: sdk_key.last_used_at,
            is_active: sdk_key.is_active,
            created_at: sdk_key.created_at,
            updated_at: sdk_key.updated_at,
            revoked_at: sdk_key.revoked_at,
            revoked_reason: sdk_key.revoked_reason,
        })
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
    let sdk_keys = api_keys::list_api_keys(&app_name, Some(true))
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

    let keys: Vec<ApiKey> = sdk_keys
        .into_iter()
        .map(|sdk_key| ApiKey {
            id: sdk_key.id.parse().unwrap_or(0),
            app_id: sdk_key.app_id.parse().unwrap_or(0),
            deployment_id: sdk_key.deployment_id.parse().unwrap_or(0),
            name: sdk_key.name,
            key_prefix: sdk_key.key_prefix,
            key_suffix: sdk_key.key_suffix,
            key_hash: String::new(),
            permissions: sdk_key.permissions,
            metadata: sdk_key.metadata,
            expires_at: sdk_key.expires_at,
            last_used_at: sdk_key.last_used_at,
            is_active: sdk_key.is_active,
            created_at: sdk_key.created_at,
            updated_at: sdk_key.updated_at,
            revoked_at: sdk_key.revoked_at,
            revoked_reason: sdk_key.revoked_reason,
        })
        .collect();

    Ok(ListApiKeysResponse { keys }.into())
}

// Create an API key
pub async fn create_api_key(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let app_name = deployment_id.to_string();

    // Get deployment to determine mode
    let deployment = queries::deployment::GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute(&app_state)
        .await
        .map_err(|_| (StatusCode::NOT_FOUND, "Deployment not found"))?;

    // Determine key_prefix based on deployment mode
    let key_prefix = match deployment.mode {
        models::DeploymentMode::Production => "sk_live",
        models::DeploymentMode::Staging => "sk_test",
    };

    // Always use default read-only scopes for console-created API keys
    let permissions = models::api_key_permissions::ApiKeyScopeHelper::scopes_to_strings(
        &models::api_key_permissions::ApiKeyScope::default_scopes(),
    );

    // Create API key using SDK
    let sdk_request = api_keys::CreateApiKeyRequest {
        name: request.name,
        key_prefix: key_prefix.to_string(),
        permissions: Some(permissions),
        expires_at: request.expires_at,
        metadata: request.metadata,
    };

    let sdk_key = api_keys::create_api_key(&app_name, sdk_request)
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

    // Manually convert SDK type to model type
    let key = ApiKey {
        id: sdk_key.key.id.parse().unwrap_or(0),
        app_id: sdk_key.key.app_id.parse().unwrap_or(0),
        deployment_id: sdk_key.key.deployment_id.parse().unwrap_or(0),
        name: sdk_key.key.name,
        key_prefix: sdk_key.key.key_prefix,
        key_suffix: sdk_key.key.key_suffix,
        key_hash: String::new(),
        permissions: sdk_key.key.permissions,
        metadata: sdk_key.key.metadata,
        expires_at: sdk_key.key.expires_at,
        last_used_at: sdk_key.key.last_used_at,
        is_active: sdk_key.key.is_active,
        created_at: sdk_key.key.created_at,
        updated_at: sdk_key.key.updated_at,
        revoked_at: sdk_key.key.revoked_at,
        revoked_reason: sdk_key.key.revoked_reason,
    };

    Ok(ApiKeyWithSecret {
        key,
        secret: sdk_key.secret,
    }
    .into())
}

// Revoke an API key
pub async fn revoke_api_key(
    Path((_, key_id)): Path<(i64, i64)>,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    // Revoke using SDK
    let sdk_request = api_keys::RevokeApiKeyRequest {
        key_id: key_id.to_string(),
        reason: request.reason,
    };

    api_keys::revoke_api_key(sdk_request).await.map_err(|e| {
        if e.to_string().contains("404") || e.to_string().contains("Not Found") {
            (StatusCode::NOT_FOUND, "API key not found".to_string())
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    })?;

    Ok(().into())
}

// Rotate an API key
pub async fn rotate_api_key(Path((_, key_id)): Path<(i64, i64)>) -> ApiResult<ApiKeyWithSecret> {
    // Rotate using SDK
    let sdk_request = api_keys::RotateApiKeyRequest {
        key_id: key_id.to_string(),
    };

    let sdk_key = api_keys::rotate_api_key(sdk_request).await.map_err(|e| {
        if e.to_string().contains("404") || e.to_string().contains("Not Found") {
            (StatusCode::NOT_FOUND, "API key not found".to_string())
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    })?;

    // Manually convert SDK type to model type
    let key = ApiKey {
        id: sdk_key.key.id.parse().unwrap_or(0),
        app_id: sdk_key.key.app_id.parse().unwrap_or(0),
        deployment_id: sdk_key.key.deployment_id.parse().unwrap_or(0),
        name: sdk_key.key.name,
        key_prefix: sdk_key.key.key_prefix,
        key_suffix: sdk_key.key.key_suffix,
        key_hash: String::new(),
        permissions: sdk_key.key.permissions,
        metadata: sdk_key.key.metadata,
        expires_at: sdk_key.key.expires_at,
        last_used_at: sdk_key.key.last_used_at,
        is_active: sdk_key.key.is_active,
        created_at: sdk_key.key.created_at,
        updated_at: sdk_key.key.updated_at,
        revoked_at: sdk_key.key.revoked_at,
        revoked_reason: sdk_key.key.revoked_reason,
    };

    Ok(ApiKeyWithSecret {
        key,
        secret: sdk_key.secret,
    }
    .into())
}
