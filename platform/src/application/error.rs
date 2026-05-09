use crate::application::response::ApiErrorResponse;
use axum::http::StatusCode;
use common::error::AppError;
use models::error::DatabaseErrorKind;
use tracing::{error, warn};

impl From<AppError> for ApiErrorResponse {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Database(_) => match error.database_kind() {
                Some(DatabaseErrorKind::NotFound) => {
                    (StatusCode::NOT_FOUND, "Resource not found").into()
                }
                Some(DatabaseErrorKind::UniqueViolation) => {
                    (StatusCode::CONFLICT, "Resource already exists").into()
                }
                Some(DatabaseErrorKind::ForeignKeyViolation) => {
                    (StatusCode::NOT_FOUND, "Related resource not found").into()
                }
                Some(DatabaseErrorKind::InvalidParameter) => {
                    (StatusCode::BAD_REQUEST, "Invalid request parameter").into()
                }
                _ => {
                    error!(error = %error, "AppError::Database translated to generic 500");
                    (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
                }
            },
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
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
            }
            AppError::Conflict(message) => (StatusCode::CONFLICT, message).into(),
            AppError::Validation(message) => (StatusCode::BAD_REQUEST, message).into(),
            AppError::Serialization(message) => {
                error!(message = %message, "AppError::Serialization");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
            }
            AppError::S3(message) => {
                error!(message = %message, "AppError::S3");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
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
