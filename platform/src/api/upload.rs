use crate::api::multipart::MultipartPayload;
use crate::{
    application::{response::ApiResult, upload as upload_app},
    middleware::RequireDeployment,
};
use common::state::AppState;
use serde::Deserialize;

use dto::json::UploadResult;

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
    let payload = MultipartPayload::parse(multipart).await?;

    for field in payload.fields() {
        let Some((bytes, extension)) = field.image_upload()? else {
            return Err((
                StatusCode::BAD_REQUEST,
                "Invalid file type. Only images are allowed.".to_string(),
            )
                .into());
        };

        file_extension = extension;
        image_buffer = bytes;
    }

    if image_buffer.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "No image data provided".to_string(),
        )
            .into());
    }

    let result = match upload_app::upload_image(
        &app_state,
        deployment_id,
        &params.image_type,
        image_buffer,
        &file_extension,
    )
    .await
    {
        Ok(result) => result,
        Err(common::error::AppError::Validation(msg)) => {
            return Err((StatusCode::BAD_REQUEST, msg).into());
        }
        Err(e) => {
            return Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into());
        }
    };

    Ok(result.into())
}
