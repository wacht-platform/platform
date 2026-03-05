use crate::api::multipart::MultipartPayload;
use crate::{application::response::ApiResult, middleware::RequireDeployment};
use common::state::AppState;
use serde::Deserialize;

use commands::{Command, UpdateDeploymentDisplaySettingsCommand, UploadToCdnCommand};
use dto::json::{DeploymentDisplaySettingsUpdates, UploadResult};

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
};

/// Path extractor that captures deployment_id (optional for backend) and image_type
#[derive(Debug, Deserialize)]
pub struct UploadPathParams {
    #[serde(rename = "deployment_id")]
    pub _deployment_id: Option<i64>,
    pub image_type: String,
}

pub async fn upload_image(
    State(app_state): State<AppState>,
    RequireDeployment(deployment_id): RequireDeployment,
    Path(params): Path<UploadPathParams>,
    multipart: Multipart,
) -> ApiResult<UploadResult> {
    let mut image_buffer: Vec<u8> = Vec::new();
    let mut file_extension = String::from("png");

    let mut updates = DeploymentDisplaySettingsUpdates::default();
    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        let Some(extension) = field.image_extension()? else {
            return Err((
                StatusCode::BAD_REQUEST,
                "Invalid file type. Only images are allowed.".to_string(),
            )
                .into());
        };

        file_extension = extension.to_string();

        image_buffer = field.bytes.clone();
    }

    if image_buffer.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "No image data provided".to_string(),
        )
            .into());
    }

    let file_path = match params.image_type.as_str() {
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
