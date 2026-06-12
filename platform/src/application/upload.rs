use commands::{DeleteFromCdnCommand, UpdateDeploymentDisplaySettingsCommand, UploadToCdnCommand};
use common::ReadConsistency;
use common::error::AppError;
use dto::json::{DeploymentDisplaySettingsUpdates, UploadResult};

use crate::application::AppState;
use common::deps;

fn build_upload_target(
    deployment_id: i64,
    image_type: &str,
    file_extension: &str,
    asset_id: i64,
) -> Result<(String, DeploymentDisplaySettingsUpdates), AppError> {
    let name = match image_type {
        "logo" => "logo",
        "favicon" => "favicon",
        "user-profile" => "user-profile",
        "org-profile" => "org-profile",
        "workspace-profile" => "workspace-profile",
        _ => {
            return Err(AppError::Validation(
                "Invalid image type. Allowed types: logo, favicon, user-profile, org-profile, workspace-profile".to_string(),
            ));
        }
    };

    // Random snowflake in the key → every upload is a unique URL. No cache to
    // bust, and clearing/replacing actually takes effect.
    let file_path = format!("deployments/{deployment_id}/{name}-{asset_id}.{file_extension}");
    let cdn_url = format!("https://cdn.wacht.services/{file_path}");

    let mut updates = DeploymentDisplaySettingsUpdates::default();
    match image_type {
        "logo" => updates.logo_image_url = Some(cdn_url),
        "favicon" => updates.favicon_image_url = Some(cdn_url),
        "user-profile" => updates.default_user_profile_image_url = Some(cdn_url),
        "org-profile" => updates.default_organization_profile_image_url = Some(cdn_url),
        "workspace-profile" => updates.default_workspace_profile_image_url = Some(cdn_url),
        _ => {}
    }

    Ok((file_path, updates))
}

/// Current stored URL for this image type, so the old object can be deleted.
async fn current_image_url(
    app_state: &AppState,
    deployment_id: i64,
    image_type: &str,
) -> Result<Option<String>, AppError> {
    queries::deployment::GetDeploymentImageUrlQuery::new(deployment_id, image_type)
        .execute_with_db(app_state.db_router.reader(ReadConsistency::Strong))
        .await
}

fn cdn_key_from_url(url: &str) -> Option<&str> {
    url.strip_prefix("https://cdn.wacht.services/")
        .filter(|key| !key.is_empty())
}

pub async fn upload_image(
    app_state: &AppState,
    deployment_id: i64,
    image_type: &str,
    image_buffer: Vec<u8>,
    file_extension: &str,
) -> Result<UploadResult, AppError> {
    let asset_id = app_state.sf.next_id()? as i64;
    let (file_path, updates) =
        build_upload_target(deployment_id, image_type, file_extension, asset_id)?;

    let previous_url = current_image_url(app_state, deployment_id, image_type).await?;

    let s3_deps = deps::from_app(app_state).s3();
    let db_redis_deps = deps::from_app(app_state).db().redis();

    let url = UploadToCdnCommand::new(file_path, image_buffer)
        .execute_with_deps(&s3_deps)
        .await?;

    UpdateDeploymentDisplaySettingsCommand::new(deployment_id, updates)
        .execute_with_deps(&db_redis_deps)
        .await?;

    // Best-effort: drop the previous (now-orphaned) object so it doesn't linger.
    if let Some(old_key) = previous_url.as_deref().and_then(cdn_key_from_url) {
        let _ = DeleteFromCdnCommand::new(old_key.to_string())
            .execute_with_deps(&s3_deps)
            .await;
    }

    Ok(UploadResult { url })
}
