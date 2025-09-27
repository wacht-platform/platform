use crate::{application::response::ApiResult, middleware::RequireDeployment};
use common::state::AppState;

use commands::{Command, UpdateDeploymentDisplaySettingsCommand, UploadToCdnCommand};
use dto::json::{DeploymentDisplaySettingsUpdates, UploadResult};

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
};

pub async fn upload_image(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path((_, image_type)): Path<(i64, String)>,
    mut multipart: Multipart,
) -> ApiResult<UploadResult> {
    let mut image_buffer: Vec<u8> = Vec::new();
    let mut file_extension = String::from("png");

    let mut updates = DeploymentDisplaySettingsUpdates::default();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let content_type = field.content_type().unwrap_or_default().to_string();

        if !content_type.starts_with("image/") {
            return Err((
                StatusCode::BAD_REQUEST,
                "Invalid file type. Only images are allowed.".to_string(),
            )
                .into());
        }

        if content_type == "image/jpeg" || content_type == "image/jpg" {
            file_extension = String::from("jpg");
        } else if content_type == "image/png" {
            file_extension = String::from("png");
        } else if content_type == "image/gif" {
            file_extension = String::from("gif");
        } else if content_type == "image/webp" {
            file_extension = String::from("webp");
        } else if content_type == "image/x-icon" || content_type == "image/vnd.microsoft.icon" {
            file_extension = String::from("ico");
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                "Unsupported image format. Supported formats: JPEG, PNG, GIF, WEBP, ICO"
                    .to_string(),
            )
                .into());
        }

        image_buffer = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
            .to_vec();
    }

    if image_buffer.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "No image data provided".to_string(),
        )
            .into());
    }

    let file_path = match image_type.as_str() {
        "logo" => {
            updates.logo_image_url = Some(format!(
                "https://cdn.wacht.services/deployments/{}/logo.{}",
                deployment_id, file_extension
            ));
            format!("deployments/{}/logo.{}", deployment_id, file_extension)
        }
        "favicon" => {
            updates.favicon_image_url = Some(format!(
                "https://cdn.wacht.services/deployments/{}/favicon.{}",
                deployment_id, file_extension
            ));
            format!("deployments/{}/favicon.{}", deployment_id, file_extension)
        }
        "user-profile" => {
            updates.default_user_profile_image_url = Some(format!(
                "https://cdn.wacht.services/deployments/{}/user-profile.{}",
                deployment_id, file_extension
            ));
            format!(
                "deployments/{}/user-profile.{}",
                deployment_id, file_extension
            )
        }
        "org-profile" => {
            updates.default_organization_profile_image_url = Some(format!(
                "https://cdn.wacht.services/deployments/{}/org-profile.{}",
                deployment_id, file_extension
            ));
            format!(
                "deployments/{}/org-profile.{}",
                deployment_id, file_extension
            )
        }
        "workspace-profile" => {
            updates.default_workspace_profile_image_url = Some(format!(
                "https://cdn.wacht.services/deployments/{}/workspace-profile.{}",
                deployment_id, file_extension
            ));
            format!(
                "deployments/{}/workspace-profile.{}",
                deployment_id, file_extension
            )
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Invalid image type. Allowed types: logo, favicon, user-profile, org-profile, workspace-profile"
                    .to_string(),
            )
                .into());
        }
    };

    let url = UploadToCdnCommand::new(file_path, image_buffer)
        .execute(&app_state)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    UpdateDeploymentDisplaySettingsCommand::new(deployment_id, updates)
        .execute(&app_state)
        .await?;

    Ok(UploadResult { url }.into())
}
