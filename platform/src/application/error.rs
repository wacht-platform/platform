use crate::application::response::ApiErrorResponse;
use axum::http::StatusCode;
use common::error::AppError;
use tracing::{error, warn};

impl From<AppError> for ApiErrorResponse {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Database(db_error) => {
                error!(error = ?db_error, "AppError::Database translated to generic 500");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
            }
            AppError::Sonyflake(sonyflake_error) => {
                error!(error = ?sonyflake_error, "AppError::Sonyflake translated to generic 500");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
            }
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message).into(),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized").into(),
            AppError::Forbidden(message) => (StatusCode::FORBIDDEN, message).into(),
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into(),
            AppError::Internal(message) => {
                error!(message = %message, "AppError::Internal");
                (StatusCode::INTERNAL_SERVER_ERROR, message).into()
            }
            AppError::Conflict(message) => (StatusCode::CONFLICT, message).into(),
            AppError::Validation(message) => (StatusCode::BAD_REQUEST, message).into(),
            AppError::Serialization(message) => {
                error!(message = %message, "AppError::Serialization");
                (StatusCode::INTERNAL_SERVER_ERROR, message).into()
            }
            AppError::S3(message) => {
                error!(message = %message, "AppError::S3");
                (StatusCode::INTERNAL_SERVER_ERROR, message).into()
            }
            AppError::External(message) => {
                warn!(message = %message, "AppError::External");
                (StatusCode::BAD_GATEWAY, message).into()
            }
            AppError::Timeout => {
                warn!("AppError::Timeout");
                (StatusCode::GATEWAY_TIMEOUT, "Operation timed out").into()
            }
        }
    }
}
