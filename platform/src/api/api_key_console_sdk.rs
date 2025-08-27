// Console-specific API key management using the Wacht SDK
// This is an example of how console endpoints can use the SDK instead of direct commands

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;

use crate::application::response::ApiResult;
use common::state::AppState;
use crate::middleware::{ConsoleDeployment, RequireDeployment};
use dto::json::api_key::{
    ApiKeyStatus, CreateApiKeyRequest as ConsoleCreateApiKeyRequest,
    ListApiKeysResponse, RevokeApiKeyRequest as ConsoleRevokeApiKeyRequest,
};
use models::{
    DeploymentMode,
    api_key::{ApiKeyApp, ApiKeyWithSecret},
    api_key_permissions::{ApiKeyScope, ApiKeyScopeHelper},
};

// Example: Get API key status using SDK
pub async fn get_api_key_status(
    State(_app_state): State<AppState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyStatus> {
    let app_name = deployment_id.to_string();
    
    // Use SDK to get API key apps
    let apps = wacht::api_keys::list_api_key_apps(Some(false))
        .await
        .map_err(|e| {
            tracing::error!("Failed to get API key apps via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get API key status")
        })?;
    
    // Find the app for this deployment
    let app = apps.iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()) == Some(&app_name))
        .cloned();
    
    // Get keys if app exists
    let keys = if app.is_some() {
        let keys = wacht::api_keys::list_api_keys(&app_name, Some(true))
            .await
            .map_err(|e| {
                tracing::error!("Failed to list API keys via SDK: {:?}", e);
                (StatusCode::INTERNAL_SERVER_ERROR, "Failed to get API keys")
            })?;
        Some(keys)
    } else {
        None
    };
    
    Ok(ApiKeyStatus {
        is_activated: app.is_some(),
        app: app.and_then(|a| serde_json::from_value(a).ok()),
        keys: keys.and_then(|k| k.into_iter()
            .map(|key| serde_json::from_value(key))
            .collect::<Result<Vec<_>, _>>()
            .ok()),
    }
    .into())
}

// Example: Activate API keys using SDK
pub async fn activate_api_keys(
    State(_app_state): State<AppState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyApp> {
    let app_name = deployment_id.to_string();
    
    // Check if already exists
    let apps = wacht::api_keys::list_api_key_apps(Some(false))
        .await
        .map_err(|e| {
            tracing::error!("Failed to list API key apps via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to check existing apps")
        })?;
    
    if apps.iter().any(|a| a.get("name").and_then(|n| n.as_str()) == Some(&app_name)) {
        return Err((
            StatusCode::BAD_REQUEST,
            "API keys already activated for this deployment",
        ).into());
    }
    
    let request = wacht::api_keys::CreateApiKeyAppRequest {
        name: app_name,
        description: Some(format!("API keys for deployment {}", deployment_id)),
        rate_limit_per_minute: Some(60),
        rate_limit_per_hour: Some(1000),
        rate_limit_per_day: Some(10000),
    };
    
    let app = wacht::api_keys::create_api_key_app(request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create API key app via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to activate API keys")
        })?;
    
    serde_json::from_value(app)
        .map(Into::into)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Invalid response format").into())
}

// Example: Deactivate API keys using SDK
pub async fn deactivate_api_keys(
    State(_app_state): State<AppState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    let app_name = deployment_id.to_string();
    
    let request = wacht::api_keys::UpdateApiKeyAppRequest {
        name: None,
        description: None,
        is_active: Some(false),
        rate_limit_per_minute: None,
        rate_limit_per_hour: None,
        rate_limit_per_day: None,
    };
    
    wacht::api_keys::update_api_key_app(&app_name, request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to deactivate API keys via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to deactivate API keys")
        })?;
    
    Ok(().into())
}

// Example: List API keys using SDK
pub async fn list_api_keys(
    State(_app_state): State<AppState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListApiKeysResponse> {
    let app_name = deployment_id.to_string();
    
    let keys = wacht::api_keys::list_api_keys(&app_name, Some(true))
        .await
        .map_err(|e| {
            tracing::error!("Failed to list API keys via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to list API keys")
        })?;
    
    // Convert JSON values to proper types
    let keys = keys
        .into_iter()
        .filter_map(|k| serde_json::from_value(k).ok())
        .collect::<Vec<_>>();
    
    Ok(ListApiKeysResponse {
        total: keys.len(),
        keys,
    }
    .into())
}

// Example: Create API key using SDK
pub async fn create_api_key(
    State(_app_state): State<AppState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<ConsoleCreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    let app_name = deployment_id.to_string();
    
    // Set default permissions based on mode
    let permissions = request.permissions.or_else(|| {
        request.mode.as_ref().map(|mode| match mode {
            DeploymentMode::Test => ApiKeyScopeHelper::test_mode_scopes()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            DeploymentMode::Live => ApiKeyScopeHelper::live_mode_scopes()
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
        })
    });
    
    // Determine key_prefix based on mode
    let key_prefix = match request.mode.as_ref() {
        Some(DeploymentMode::Live) => "sk_live",
        Some(DeploymentMode::Test) | None => "sk_test",
    };

    let sdk_request = wacht::api_keys::CreateApiKeyRequest {
        name: request.name,
        key_prefix: key_prefix.to_string(),
        permissions,
        expires_at: request.expires_at,
        metadata: request.metadata,
    };
    
    let key = wacht::api_keys::create_api_key(&app_name, sdk_request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create API key via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to create API key")
        })?;
    
    Ok(key.into())
}

// Example: Revoke API key using SDK
pub async fn revoke_api_key(
    State(_app_state): State<AppState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(key_id): Path<i64>,
) -> ApiResult<()> {
    let request = wacht::api_keys::RevokeApiKeyRequest {
        key_id,
        reason: Some("Revoked via console".to_string()),
    };
    
    wacht::api_keys::revoke_api_key(request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to revoke API key via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to revoke API key")
        })?;
    
    Ok(().into())
}

// Example: Rotate API key using SDK
pub async fn rotate_api_key(
    State(_app_state): State<AppState>,
    ConsoleDeployment(_console_deployment_id): ConsoleDeployment,
    RequireDeployment(_deployment_id): RequireDeployment,
    Path(key_id): Path<i64>,
) -> ApiResult<ApiKeyWithSecret> {
    let request = wacht::api_keys::RotateApiKeyRequest {
        key_id,
    };
    
    let new_key = wacht::api_keys::rotate_api_key(request)
        .await
        .map_err(|e| {
            tracing::error!("Failed to rotate API key via SDK: {:?}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "Failed to rotate API key")
        })?;
    
    Ok(new_key.into())
}

// Note: The get_api_key_stats endpoint is console-specific and would remain as is