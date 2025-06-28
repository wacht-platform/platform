use crate::application::response::ApiErrorResponse;
use axum::http::StatusCode;
use shared::error::AppError;

impl From<AppError> for ApiErrorResponse {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Database(_) | AppError::Sonyflake(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
            }
            AppError::NotFound(message) => (StatusCode::NOT_FOUND, message).into(),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized").into(),
            AppError::BadRequest(message) => (StatusCode::BAD_REQUEST, message).into(),
            AppError::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message).into(),
            AppError::Validation(message) => (StatusCode::BAD_REQUEST, message).into(),
            AppError::Serialization(message) => (StatusCode::INTERNAL_SERVER_ERROR, message).into(),
            AppError::S3(message) => (StatusCode::INTERNAL_SERVER_ERROR, message).into(),
            AppError::External(message) => (StatusCode::BAD_GATEWAY, message).into(),
        }
    }
}
