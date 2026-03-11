use commands::{UpdateDeploymentDisplaySettingsCommand, UploadToCdnCommand};
use common::error::AppError;
use dto::json::{DeploymentDisplaySettingsUpdates, UploadResult};

use crate::application::AppState;
use common::deps;

fn build_upload_target(
    deployment_id: i64,
    image_type: &str,
    file_extension: &str,
) -> Result<(String, DeploymentDisplaySettingsUpdates), AppError> {
    let mut updates = DeploymentDisplaySettingsUpdates::default();
    let (cdn_url, file_path) = match image_type {
        "logo" => (
            format!(
                "https://cdn.wacht.services/deployments/{}/logo.{}",
                deployment_id, file_extension
            ),
            format!("deployments/{}/logo.{}", deployment_id, file_extension),
        ),
        "favicon" => (
            format!(
                "https://cdn.wacht.services/deployments/{}/favicon.{}",
                deployment_id, file_extension
            ),
            format!("deployments/{}/favicon.{}", deployment_id, file_extension),
        ),
        "user-profile" => (
            format!(
                "https://cdn.wacht.services/deployments/{}/user-profile.{}",
                deployment_id, file_extension
            ),
            format!(
                "deployments/{}/user-profile.{}",
                deployment_id, file_extension
            ),
        ),
        "org-profile" => (
            format!(
                "https://cdn.wacht.services/deployments/{}/org-profile.{}",
                deployment_id, file_extension
            ),
            format!(
                "deployments/{}/org-profile.{}",
                deployment_id, file_extension
            ),
        ),
        "workspace-profile" => (
            format!(
                "https://cdn.wacht.services/deployments/{}/workspace-profile.{}",
                deployment_id, file_extension
            ),
            format!(
                "deployments/{}/workspace-profile.{}",
                deployment_id, file_extension
            ),
        ),
        _ => {
            return Err(AppError::Validation(
                "Invalid image type. Allowed types: logo, favicon, user-profile, org-profile, workspace-profile".to_string(),
            ));
        }
    };

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

pub async fn upload_image(
    app_state: &AppState,
    deployment_id: i64,
    image_type: &str,
    image_buffer: Vec<u8>,
    file_extension: &str,
) -> Result<UploadResult, AppError> {
    let (file_path, updates) = build_upload_target(deployment_id, image_type, file_extension)?;
    let s3_deps = deps::from_app(app_state).s3();
    let db_redis_deps = deps::from_app(app_state).db().redis();

    let url = UploadToCdnCommand::new(file_path, image_buffer)
        .execute_with_deps(&s3_deps)
        .await?;

    UpdateDeploymentDisplaySettingsCommand::new(deployment_id, updates)
        .execute_with_deps(&db_redis_deps)
        .await?;

    Ok(UploadResult { url })
}
