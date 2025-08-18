// Console-specific API key management functions
// API key apps are stored in the console's database
// Each customer deployment can have one API key app (named after their deployment_id)

use axum::extract::{Json, Path, State};
use axum::http::StatusCode;

use crate::application::{HttpState, response::ApiResult};
use crate::middleware::{RequireDeployment, ConsoleDeployment};
use commands::{
    Command,
    api_key_app::{CreateApiKeyAppCommand, UpdateApiKeyAppCommand},
    api_key::{CreateApiKeyCommand, RevokeApiKeyCommand, RotateApiKeyCommand},
};
use dto::json::api_key::{
    ApiKeyStatus, ApiKeyStats, ListApiKeysResponse,
    CreateApiKeyRequest, RevokeApiKeyRequest,
};
use models::{
    api_key::{ApiKeyApp, ApiKeyWithSecret},
    DeploymentMode,
};
use queries::{
    Query as QueryTrait,
    api_key::{GetApiKeyAppByNameQuery, GetApiKeysByAppQuery},
};

// Get API key status for a deployment
pub async fn get_api_key_status(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyStatus> {
    let app_name = deployment_id.to_string();
    
    let app = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?;
    
    let keys = if let Some(ref app) = app {
        Some(
            GetApiKeysByAppQuery::new(app.id, console_deployment_id)
                .with_inactive(true)
                .execute(&app_state)
                .await?
        )
    } else {
        None
    };
    
    Ok(ApiKeyStatus {
        is_activated: app.is_some(),
        app,
        keys,
    }.into())
}

// Activate API keys for a deployment
pub async fn activate_api_keys(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyApp> {
    let app_name = deployment_id.to_string();
    
    // Check if already exists
    let existing = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name.clone())
        .execute(&app_state)
        .await?;
    
    if existing.is_some() {
        return Err((StatusCode::BAD_REQUEST, "API keys already activated for this deployment").into());
    }
    
    // Create API key app in console's database
    let mut command = CreateApiKeyAppCommand::new(console_deployment_id, app_name);
    command = command.with_description(format!("API keys for deployment {}", deployment_id));
    
    // Set default rate limits
    command = command.with_rate_limits(60, 1000);
    
    let app = command.execute(&app_state).await?;
    Ok(app.into())
}

// Deactivate API keys for a deployment
pub async fn deactivate_api_keys(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<()> {
    let app_name = deployment_id.to_string();
    
    let app = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;
    
    let command = UpdateApiKeyAppCommand {
        app_id: app.id,
        deployment_id: console_deployment_id,
        name: None,
        description: None,
        is_active: Some(false),
        rate_limit_per_minute: None,
        rate_limit_per_hour: None,
    };
    
    command.execute(&app_state).await?;
    Ok(().into())
}

// Get API key statistics
pub async fn get_api_key_stats(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ApiKeyStats> {
    let app_name = deployment_id.to_string();
    
    let app = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found. Please activate API keys first."))?;
    
    let keys = GetApiKeysByAppQuery::new(app.id, console_deployment_id)
        .with_inactive(true)
        .execute(&app_state)
        .await?;
    
    let total_keys = keys.len() as i64;
    let active_keys = keys.iter().filter(|k| k.is_active).count() as i64;
    let revoked_keys = keys.iter().filter(|k| !k.is_active).count() as i64;
    
    // Calculate keys used in last 24 hours
    let now = chrono::Utc::now();
    let twenty_four_hours_ago = now - chrono::Duration::hours(24);
    let keys_used_24h = keys.iter()
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
    }.into())
}

// List API keys
pub async fn list_api_keys(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
) -> ApiResult<ListApiKeysResponse> {
    let app_name = deployment_id.to_string();
    
    let app = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found. Please activate API keys first."))?;
    
    let keys = GetApiKeysByAppQuery::new(app.id, console_deployment_id)
        .with_inactive(true)
        .execute(&app_state)
        .await?;
    
    Ok(ListApiKeysResponse { keys }.into())
}

// Create an API key
pub async fn create_api_key(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Json(request): Json<CreateApiKeyRequest>,
) -> ApiResult<ApiKeyWithSecret> {
    use queries::deployment::GetDeploymentWithSettingsQuery;
    
    let app_name = deployment_id.to_string();
    
    // Get the app
    let app = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found. Please activate API keys first."))?;
    
    // Get deployment to determine key prefix
    let deployment = GetDeploymentWithSettingsQuery::new(deployment_id)
        .execute(&app_state)
        .await?;
    
    // Automatically determine key prefix based on deployment mode
    let key_prefix = match deployment.mode {
        DeploymentMode::Production => "sk_live",
        DeploymentMode::Staging => "sk_test",
    };
    
    let mut command = CreateApiKeyCommand::new(
        app.id,
        console_deployment_id,
        request.name,
        key_prefix.to_string(),
    );
    
    if let Some(permissions) = request.permissions {
        command = command.with_permissions(permissions);
    }
    
    if let Some(expires_at) = request.expires_at {
        command = command.with_expiration(expires_at);
    }
    
    if let Some(metadata) = request.metadata {
        command.metadata = Some(metadata);
    }
    
    let key_with_secret = command.execute(&app_state).await?;
    Ok(key_with_secret.into())
}

// Revoke an API key
pub async fn revoke_api_key(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((_, key_id)): Path<(i64, i64)>,
    Json(request): Json<RevokeApiKeyRequest>,
) -> ApiResult<()> {
    let app_name = deployment_id.to_string();
    
    // Verify app exists
    let _app = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;
    
    let command = RevokeApiKeyCommand {
        key_id,
        deployment_id: console_deployment_id,
        reason: request.reason,
    };
    
    command.execute(&app_state).await?;
    Ok(().into())
}

// Rotate an API key
pub async fn rotate_api_key(
    State(app_state): State<HttpState>,
    ConsoleDeployment(console_deployment_id): ConsoleDeployment,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((_, key_id)): Path<(i64, i64)>,
) -> ApiResult<ApiKeyWithSecret> {
    let app_name = deployment_id.to_string();
    
    // Verify app exists
    let _app = GetApiKeyAppByNameQuery::new(console_deployment_id, app_name)
        .execute(&app_state)
        .await?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "API key app not found"))?;
    
    let command = RotateApiKeyCommand {
        key_id,
        deployment_id: console_deployment_id,
    };
    
    let key_with_secret = command.execute(&app_state).await?;
    Ok(key_with_secret.into())
}