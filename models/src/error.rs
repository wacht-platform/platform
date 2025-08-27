use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Sonyflake error: {0}")]
    Sonyflake(#[from] sonyflake::Error),
    #[error("Resource not found")]
    NotFound(String),
    #[error("Unauthorized access")]
    Unauthorized,
    #[error("Bad request: {0}")]
    BadRequest(String),
    #[error("Internal error: {0}")]
    Internal(String),
    #[error("Validation error: {0}")]
    Validation(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("S3 error: {0}")]
    S3(String),
    #[error("External service error: {0}")]
    External(String),
}

impl From<serde_json::Error> for AppError {
    fn from(error: serde_json::Error) -> Self {
        AppError::Serialization(error.to_string())
    }
}

impl From<redis::RedisError> for AppError {
    fn from(error: redis::RedisError) -> Self {
        AppError::Internal(error.to_string())
    }
}

impl From<clickhouse::error::Error> for AppError {
    fn from(error: clickhouse::error::Error) -> Self {
        AppError::Database(sqlx::Error::Protocol(error.to_string()))
    }
}
