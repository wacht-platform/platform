use crate::application::response::ApiErrorResponse;
use axum::http::StatusCode;
use common::error::AppError;
use sqlx::Error as SqlxError;
use tracing::{error, warn};

fn translate_database_error(error: SqlxError) -> ApiErrorResponse {
    match error {
        SqlxError::RowNotFound => (StatusCode::NOT_FOUND, "Resource not found").into(),
        SqlxError::Database(db_error) => match db_error.code().as_deref() {
            Some("23505") => (StatusCode::CONFLICT, "Resource already exists").into(),
            Some("23503") => (StatusCode::NOT_FOUND, "Related resource not found").into(),
            Some("22P02") => (StatusCode::BAD_REQUEST, "Invalid request parameter").into(),
            _ => {
                error!(error = ?db_error, "AppError::Database translated to generic 500");
                (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
            }
        },
        other => {
            error!(error = ?other, "AppError::Database translated to generic 500");
            (StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong").into()
        }
    }
}

impl From<AppError> for ApiErrorResponse {
    fn from(error: AppError) -> Self {
        match error {
            AppError::Database(db_error) => translate_database_error(db_error),
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
